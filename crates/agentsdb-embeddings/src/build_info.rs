#[allow(clippy::all, dead_code)]
mod generated {
    include!(concat!(env!("OUT_DIR"), "/dep_versions.rs"));
}

#[allow(dead_code)]
fn join(versions: &[&str]) -> Option<String> {
    match versions {
        [] => None,
        [one] => Some((*one).to_string()),
        many => Some(many.join(",")),
    }
}

#[allow(dead_code)]
pub fn runtime_version_fastembed() -> Option<String> {
    let fastembed = join(generated::FASTEMBED_VERSIONS)?;
    let hf_hub = join(generated::HF_HUB_VERSIONS);
    match hf_hub {
        Some(hf_hub) => Some(format!("fastembed {fastembed}; hf-hub {hf_hub}")),
        None => Some(format!("fastembed {fastembed}")),
    }
}

#[allow(dead_code)]
pub fn runtime_version_http() -> Option<String> {
    let ureq = join(generated::UREQ_VERSIONS)?;
    Some(format!("ureq {ureq}"))
}

#[allow(dead_code)]
pub fn runtime_version_candle() -> Option<String> {
    let candle_core = join(generated::CANDLE_CORE_VERSIONS)?;
    let candle_nn = join(generated::CANDLE_NN_VERSIONS);
    let candle_transformers = join(generated::CANDLE_TRANSFORMERS_VERSIONS);
    let tokenizers = join(generated::TOKENIZERS_VERSIONS);
    let hf_hub = join(generated::HF_HUB_VERSIONS);

    let mut parts = vec![format!("candle-core {candle_core}")];
    if let Some(v) = candle_nn {
        parts.push(format!("candle-nn {v}"));
    }
    if let Some(v) = candle_transformers {
        parts.push(format!("candle-transformers {v}"));
    }
    if let Some(v) = tokenizers {
        parts.push(format!("tokenizers {v}"));
    }
    if let Some(v) = hf_hub {
        parts.push(format!("hf-hub {v}"));
    }
    Some(parts.join("; "))
}
