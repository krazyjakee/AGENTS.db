pub(crate) fn cmd_web(root: &str, bind: &str) -> anyhow::Result<()> {
    // Implements the `web` command, which launches a local Web UI for browsing and editing writable layers.
    //
    // This function delegates to the `agentsdb_web::serve` function to start the web server.
    agentsdb_web::serve(root, bind)
}
