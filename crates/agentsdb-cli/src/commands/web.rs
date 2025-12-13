pub(crate) fn cmd_web(root: &str, bind: &str) -> anyhow::Result<()> {
    agentsdb_web::serve(root, bind)
}
