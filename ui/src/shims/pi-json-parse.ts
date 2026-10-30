/**
 * Local copy of the small streaming JSON reader MessageView uses. It keeps a
 * partially streamed tool argument readable without pulling in the pi SDK.
 */
export function parseStreamingJson<T = unknown>(text: string): T | undefined {
  const trimmed = text.trim();
  if (!trimmed) return undefined;
  try {
    return JSON.parse(trimmed) as T;
  } catch {
    // Close strings, arrays and objects that a stream has not finished yet.
    let candidate = trimmed;
    let inString = false;
    let escaped = false;
    const stack: string[] = [];
    for (const char of candidate) {
      if (inString) {
        if (escaped) escaped = false;
        else if (char === "\\") escaped = true;
        else if (char === '"') inString = false;
        continue;
      }
      if (char === '"') inString = true;
      else if (char === "{" || char === "[") stack.push(char);
      else if (char === "}" || char === "]") stack.pop();
    }
    if (inString) candidate += '"';
    for (let i = stack.length - 1; i >= 0; i--) candidate += stack[i] === "{" ? "}" : "]";
    try {
      return JSON.parse(candidate) as T;
    } catch {
      return undefined;
    }
  }
}
