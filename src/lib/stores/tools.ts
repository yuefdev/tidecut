import { writable } from "svelte/store";

export type ToolId = "select" | "cut" | "text";

export const activeTool = writable<ToolId>("select");

export function setActiveTool(tool: ToolId): void {
  activeTool.set(tool);
}

export function resetTool(): void {
  activeTool.set("select");
}
