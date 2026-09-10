use std::fs::{self, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::{env, process::Command};

use fs2::FileExt;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::{AppError, Result};

const ENV_PATH: &str = "LGR_AGENT_BUDGET_PATH";
pub const MAX_RESPONSES: u64 = 16;
pub const MAX_RETURNED_BYTES: u64 = 192 * 1024;

#[derive(Debug, Deserialize, Serialize)]
struct BudgetState {
    version: u32,
    responses: u64,
    returned_bytes: u64,
    max_responses: u64,
    max_returned_bytes: u64,
}

pub struct AgentBudget {
    path: PathBuf,
}

impl AgentBudget {
    pub fn start(data_dir: &Path) -> Result<Self> {
        let directory = data_dir.join("agent-budgets");
        fs::create_dir_all(&directory)?;
        let path = directory.join(format!("run-{}.json", Uuid::new_v4().simple()));
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .mode(0o600)
            .open(&path)?;
        serde_json::to_writer(
            &mut file,
            &BudgetState {
                version: 1,
                responses: 0,
                returned_bytes: 0,
                max_responses: MAX_RESPONSES,
                max_returned_bytes: MAX_RETURNED_BYTES,
            },
        )?;
        file.flush()?;
        Ok(Self { path })
    }

    pub fn configure(&self, command: &mut Command) {
        command.env(ENV_PATH, &self.path);
    }
}

impl Drop for AgentBudget {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

pub fn charge_response(bytes: u64) -> Result<()> {
    let Some(path) = env::var_os(ENV_PATH) else {
        return Ok(());
    };
    charge_path(Path::new(&path), bytes)
}

fn charge_path(path: &Path, bytes: u64) -> Result<()> {
    let mut file = OpenOptions::new().read(true).write(true).open(path)?;
    file.lock_exclusive()?;
    let mut encoded = String::new();
    file.read_to_string(&mut encoded)?;
    let mut state: BudgetState = serde_json::from_str(&encoded)?;
    state.responses += 1;
    let over_calls = state.responses > state.max_responses;
    let over_bytes = state.returned_bytes.saturating_add(bytes) > state.max_returned_bytes;
    if !over_calls && !over_bytes {
        state.returned_bytes += bytes;
    }
    file.seek(SeekFrom::Start(0))?;
    file.set_len(0)?;
    serde_json::to_writer(&mut file, &state)?;
    file.flush()?;
    FileExt::unlock(&file)?;

    if over_calls || over_bytes {
        return Err(AppError::InvalidInput {
            code: "agent_response_budget_exceeded",
            message: format!(
                "ranking evidence budget exhausted ({} responses or {} bytes); stop reading and finalize with current evidence",
                state.max_responses, state.max_returned_bytes
            ),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn caps_response_count() {
        let directory = tempfile::tempdir().unwrap();
        let budget = AgentBudget::start(directory.path()).unwrap();
        for _ in 0..MAX_RESPONSES {
            charge_path(&budget.path, 1).unwrap();
        }
        let error = charge_path(&budget.path, 1).unwrap_err();
        assert_eq!(error.code(), "agent_response_budget_exceeded");
    }

    #[test]
    fn rejects_response_that_would_cross_byte_cap() {
        let directory = tempfile::tempdir().unwrap();
        let budget = AgentBudget::start(directory.path()).unwrap();
        charge_path(&budget.path, MAX_RETURNED_BYTES).unwrap();
        let error = charge_path(&budget.path, 1).unwrap_err();
        assert_eq!(error.code(), "agent_response_budget_exceeded");
    }
}
