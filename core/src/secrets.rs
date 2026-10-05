use crate::manifest::SecretReference;
use std::path::Path;

pub(crate) fn resolve<F>(
    reference: &SecretReference,
    manifest_dir: &Path,
    get_env: &F,
) -> Result<String, &'static str>
where
    F: Fn(&str) -> Result<String, std::env::VarError>,
{
    let value = match reference {
        SecretReference::Environment { env } => {
            if env.is_empty() || !env.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
                return Err("secret environment reference is invalid");
            }
            get_env(env).map_err(|_| "required secret environment variable is unavailable")?
        }
        SecretReference::File { file } => std::fs::read_to_string(manifest_dir.join(file))
            .map_err(|_| "required secret file is unavailable")?
            .trim_end_matches(['\r', '\n'])
            .to_owned(),
    };
    if value.is_empty() {
        return Err("resolved secret must not be empty");
    }
    Ok(value)
}
