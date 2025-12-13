use crate::types::ValidateJson;

pub(crate) fn cmd_validate(path: &str, json: bool) -> anyhow::Result<()> {
    let res = agentsdb_format::LayerFile::open(path);
    match res {
        Ok(_) => {
            if json {
                let out = ValidateJson {
                    ok: true,
                    path,
                    error: None,
                };
                println!("{}", serde_json::to_string_pretty(&out)?);
            } else {
                println!("OK: {path}");
            }
            Ok(())
        }
        Err(e) => {
            if json {
                let out = ValidateJson {
                    ok: false,
                    path,
                    error: Some(e.to_string()),
                };
                println!("{}", serde_json::to_string_pretty(&out)?);
                std::process::exit(1);
            } else {
                anyhow::bail!("INVALID: {path}: {e}");
            }
        }
    }
}
