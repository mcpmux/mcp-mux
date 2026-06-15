import { invoke } from '@tauri-apps/api/core';

/** Add McpMux to VS Code via deep link. */
export async function addToVscode(gatewayUrl: string): Promise<void> {
  return invoke('add_to_vscode', { gatewayUrl });
}

/** Add McpMux to Cursor via deep link. */
export async function addToCursor(gatewayUrl: string): Promise<void> {
  return invoke('add_to_cursor', { gatewayUrl });
}
