import { createMcpServer, openStore, runMcpServer } from "./mcp-runtime";

export { createMcpServer, openStore, runMcpServer };

if (require.main === module) {
  runMcpServer().catch((err) => {
    console.error("MCP server error:", err);
    process.exit(1);
  });
}
