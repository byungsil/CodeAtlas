import { createMcpServer } from "../mcp";

type JsonRpcMessage = { jsonrpc: string; id?: number; method: string; params?: any };

export async function mcpCall(messages: JsonRpcMessage[], dataDir: string): Promise<any[]> {
  const { server, close } = createMcpServer(dataDir);
  const core = server.server as any;
  const requestHandlers: Map<string, Function> = core._requestHandlers;
  const notificationHandlers: Map<string, Function> = core._notificationHandlers;
  const responses: any[] = [];

  try {
    for (const message of messages) {
      if (typeof message.id === "number") {
        const handler = requestHandlers.get(message.method);
        if (!handler) {
          throw new Error(`Missing MCP request handler for ${message.method}`);
        }
        const result = await handler(message, { sessionId: "test-session" });
        responses.push({ jsonrpc: "2.0", id: message.id, result });
      } else {
        const handler = notificationHandlers.get(message.method);
        if (handler) {
          await handler(message);
        }
      }
    }
  } finally {
    close();
  }

  return responses;
}
