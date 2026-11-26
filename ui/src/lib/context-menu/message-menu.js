/**
 * Markdown source for the "Copy as Markdown" message action.
 *
 * Assistant text blocks already hold Markdown; user content is either a
 * Markdown string or text/image blocks. Thinking and tool calls are left out:
 * they are not part of what the sender wrote.
 *
 * @param {{ role?: string, content?: unknown }} message
 * @returns {string}
 */
export function messageMarkdown(message) {
  const content = message?.content;
  if (typeof content === "string") return content;
  if (!Array.isArray(content)) return "";
  return content
    .filter((block) => block && block.type === "text" && typeof block.text === "string")
    .map((block) => block.text)
    .join("\n\n");
}
