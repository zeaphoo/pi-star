// Types-only shims for the parts of the pi SDK surface the copied UI references.
// pi-star implements the runtime in Rust; these keep copied client code
// typecheckable without shipping the SDK.
declare module "@earendil-works/pi-coding-agent" {
  export type ThinkingLevel = "off" | "minimal" | "low" | "medium" | "high" | "xhigh" | "max";
  export type McpExposure = string;
  export type McpServerConfig = Record<string, any>;
  export type McpServerEntry = Record<string, any>;
  export type McpTransportFactory = any;
  export type ResourceDiagnostic = { message?: string; type?: string; path?: string };
  export type JsonAgentSessionEvent = any;
  export type AgentSessionEvent = any;
  export type BashOperations = any;
  export type SessionManager = any;
  export type SettingsManager = any;
  export type SlashCommandInfo = any;
  export type Theme = any;
  export type AgentSession = any;
  export type Agent = any;
  export type InlineExtension = any;
  export type ToolInfo = any;
  export type ExtensionContext = any;
  export type ToolExposure = any;
  export const defineTool: any;
  export type LoadExtensionsResult = any;
  export type ModelRuntime = any;
  export type DefaultResourceLoader = any;
  export type Skill = any;
  export const getAgentDir: (...args: any[]) => string;
  export const parseFrontmatter: (...args: any[]) => any;
  export const formatSkillsForPrompt: (...args: any[]) => string;
  export const CONFIG_DIR_NAME: string;
  export const ModelRegistry: any;
  export const SessionManager: any;
  export const SettingsManager: any;
  export const DefaultResourceLoader: any;
  export const createAgentSessionServices: any;
  export const createAgentSessionFromServices: any;
  export const initTheme: any;
}
declare module "@earendil-works/pi-agent-core" {
  export type ThinkingLevel = "off" | "minimal" | "low" | "medium" | "high" | "xhigh" | "max";
  export type AgentMessage = any;
  export type Agent = any;
}
declare module "@earendil-works/pi-ai" {
  export type ImageContent = { type: "image"; [key: string]: any };
  export type TextContent = { type: "text"; text: string };
  export type Api = any;
  export type Model<T = any> = any;
  export type Credential = any;
  export type AssistantMessage = any;
  export const Type: any;
  export const getSupportedThinkingLevels: (...args: any[]) => string[];
  export const completeSimple: any;
}
declare module "@earendil-works/pi-ai/providers/all" {
  export const getBuiltinProviders: (...args: any[]) => any[];
}
declare module "@earendil-works/pi-ai/compat" {
  export const completeSimple: any;
  export type AssistantMessage = any;
}
declare module "@earendil-works/pi-ai/utils/json-parse" {
  export function parseStreamingJson<T = unknown>(text: string): T | undefined;
}
