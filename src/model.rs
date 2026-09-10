use std::fmt;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::{AppError, ErrorBody};

pub const API_SCHEMA_VERSION: u32 = 1;

macro_rules! id_type {
    ($name:ident, $prefix:literal) => {
        #[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            pub fn new() -> Self {
                Self(format!("{}{}", $prefix, Uuid::new_v4().simple()))
            }
            pub fn parse(value: impl Into<String>) -> Result<Self, String> {
                let value = value.into();
                if value.starts_with($prefix) && value.len() > $prefix.len() {
                    Ok(Self(value))
                } else {
                    Err(format!("expected identifier beginning with {}", $prefix))
                }
            }
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }
        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(f)
            }
        }
    };
}

id_type!(SessionId, "ses_");
id_type!(SnapshotId, "snp_");
id_type!(GraphRevisionId, "grf_");
id_type!(ContextId, "ctx_");
id_type!(CommentId, "cmt_");

#[derive(Debug, Serialize)]
pub struct Envelope<T: Serialize> {
    pub schema_version: u32,
    pub request_id: String,
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<T>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub errors: Vec<ErrorBody>,
}

impl<T: Serialize> Envelope<T> {
    pub fn success(result: T) -> Self {
        Self {
            schema_version: API_SCHEMA_VERSION,
            request_id: Uuid::new_v4().to_string(),
            ok: true,
            result: Some(result),
            errors: Vec::new(),
        }
    }

    pub fn failure(error: &AppError) -> Self {
        Self {
            schema_version: API_SCHEMA_VERSION,
            request_id: Uuid::new_v4().to_string(),
            ok: false,
            result: None,
            errors: vec![error.into()],
        }
    }
}
