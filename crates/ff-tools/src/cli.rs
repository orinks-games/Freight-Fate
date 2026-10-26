//! A small argument reader: `--name value`, `--name=value`, `--flag`, and
//! positionals. The tools take a handful of options each; a full parser
//! would cost the CI gates more compile time than it saves.

use std::collections::HashMap;
use std::path::PathBuf;

/// What went wrong reading the command line, said to the person who typed it.
#[derive(Debug)]
pub struct UsageError(pub String);

impl std::fmt::Display for UsageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for UsageError {}

/// The parsed options of one command.
#[derive(Debug, Default)]
pub struct Args {
    values: HashMap<String, Vec<String>>,
    flags: Vec<String>,
    pub positionals: Vec<String>,
}

/// How one command's options are spelled.
pub struct Spec<'a> {
    /// Options that take a value.
    pub values: &'a [&'a str],
    /// Options that are on or off.
    pub flags: &'a [&'a str],
}

impl Args {
    pub fn parse(argv: &[String], spec: &Spec) -> Result<Self, UsageError> {
        let mut args = Args::default();
        let mut i = 0;
        while i < argv.len() {
            let arg = &argv[i];
            if arg == "--" {
                args.positionals.extend(argv[i + 1..].iter().cloned());
                break;
            }
            if let Some(name) = arg.strip_prefix("--") {
                let (name, inline) = match name.split_once('=') {
                    Some((n, v)) => (n, Some(v.to_string())),
                    None => (name, None),
                };
                if spec.flags.contains(&name) {
                    if inline.is_some() {
                        return Err(UsageError(format!("--{name} takes no value")));
                    }
                    args.flags.push(name.to_string());
                } else if spec.values.contains(&name) {
                    let value = match inline {
                        Some(v) => v,
                        None => {
                            i += 1;
                            argv.get(i)
                                .cloned()
                                .ok_or_else(|| UsageError(format!("--{name} needs a value")))?
                        }
                    };
                    args.values.entry(name.to_string()).or_default().push(value);
                } else {
                    return Err(UsageError(format!("unknown option --{name}")));
                }
            } else {
                args.positionals.push(arg.clone());
            }
            i += 1;
        }
        Ok(args)
    }

    pub fn flag(&self, name: &str) -> bool {
        self.flags.iter().any(|f| f == name)
    }

    /// The last value given for `name`.
    pub fn value(&self, name: &str) -> Option<&str> {
        self.values
            .get(name)
            .and_then(|v| v.last())
            .map(String::as_str)
    }

    pub fn string(&self, name: &str, default: &str) -> String {
        self.value(name).unwrap_or(default).to_string()
    }

    pub fn required(&self, name: &str) -> Result<String, UsageError> {
        self.value(name)
            .map(str::to_string)
            .ok_or_else(|| UsageError(format!("--{name} is required")))
    }

    pub fn path(&self, name: &str) -> Option<PathBuf> {
        self.value(name).map(PathBuf::from)
    }

    pub fn number<T: std::str::FromStr>(&self, name: &str, default: T) -> Result<T, UsageError> {
        match self.value(name) {
            None => Ok(default),
            Some(v) => v
                .parse()
                .map_err(|_| UsageError(format!("--{name} expects a number, not {v:?}"))),
        }
    }

    pub fn choice(
        &self,
        name: &str,
        choices: &[&str],
        default: &str,
    ) -> Result<String, UsageError> {
        let value = self.string(name, default);
        if choices.contains(&value.as_str()) {
            Ok(value)
        } else {
            Err(UsageError(format!(
                "--{name} must be one of {}, not {value:?}",
                choices.join(", ")
            )))
        }
    }
}
