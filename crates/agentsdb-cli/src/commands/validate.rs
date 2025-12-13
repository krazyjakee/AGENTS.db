use crate::types::ValidateJson;

pub(crate) fn cmd_validate(path: &str, json: bool) -> anyhow::Result<()> {
    let err = agentsdb_format::LayerFile::open(path).err();
    if json {
        let out = ValidateJson {
            ok: err.is_none(),
            path,
            error: err.map(|e| e.to_string()),
        };
        println!("{}", serde_json::to_string_pretty(&out)?);
        if out.ok {
            Ok(())
        } else {
            std::process::exit(1);
        }
    } else if let Some(e) = err {
        anyhow::bail!("INVALID: {path}: {e}");
    } else {
        println!("OK: {path}");
        Ok(())
    }
}
