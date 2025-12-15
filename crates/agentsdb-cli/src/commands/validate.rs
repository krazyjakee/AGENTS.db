use crate::types::ValidateJson;

pub(crate) fn cmd_validate(path: &str, json: bool) -> anyhow::Result<()> {
    // Implements the `validate` command, which validates that a layer file is readable and well-formed.
    //
    // This function attempts to open and parse the specified layer file, reporting success or any errors encountered.
    // Output can be formatted as human-readable text or JSON.
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
