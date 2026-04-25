import { invoke } from '@tauri-apps/api/core';

/**
 * A Client represents an AI assistant (Cursor, VS Code, Claude, etc.).
 *
 * Identity only — routing is decided at session time by the gateway's
 * FeatureSetResolver (WorkspaceBinding → Space default FS), not per client.
 */
export interface Client {
  id: string;
  name: string;
  client_type: string;
  last_seen: string | null;
}

/** Input for creating a client. */
export interface CreateClientInput {
  name: string;
  client_type: string;
}

/** List all clients. */
export async function listClients(): Promise<Client[]> {
  return invoke('list_clients');
}

/** Get a client by ID. */
export async function getClient(id: string): Promise<Client | null> {
  return invoke('get_client', { id });
}

/** Create a new client. */
export async function createClient(input: CreateClientInput): Promise<Client> {
  return invoke('create_client', { input });
}

/** Delete a client. */
export async function deleteClient(id: string): Promise<void> {
  return invoke('delete_client', { id });
}

/** Initialize preset clients (Cursor, VS Code, Claude). */
export async function initPresetClients(): Promise<void> {
  return invoke('init_preset_clients');
}
