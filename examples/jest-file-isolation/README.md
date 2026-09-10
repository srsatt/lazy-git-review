# Jest file-isolation coverage

Copy `test_profiles.jest-file` from `settings.fragment.json` into `.lgr/settings.json`, then run one test file per LGR run:

```sh
lgr tests run SESSION --profile jest-file --select tests/add.test.js
lgr tests run SESSION --profile jest-file --select tests/subtract.test.js
```

The profile's explicit preparation step installs the lockfile-pinned dependencies with lifecycle scripts disabled inside each disposable captured workspace. Browsing never performs this step. The `{selection}` argument is expanded as argv, never through a shell. Each Jest invocation uses `--runTestsByPath` and writes a fresh Istanbul report, so LGR can attribute executed ranges to one test file. It does not claim which `test(...)` case produced a hit or that a hit proves an assertion.
