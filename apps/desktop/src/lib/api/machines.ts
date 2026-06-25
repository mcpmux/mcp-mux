import { apiCall } from './transport';

/** A registered machine (homelab box, laptop, cloud agent). */
export interface Machine {
  id: string;
  name: string;
  icon: string | null;
  hostname: string | null;
  created_at: string;
  updated_at: string;
}

export interface CreateMachineInput {
  name: string;
  icon?: string | null;
  hostname?: string | null;
}

export interface UpdateMachineInput {
  name?: string;
  icon?: string | null;
  hostname?: string | null;
}

/** List all registered machines. */
export async function listMachines(): Promise<Machine[]> {
  return apiCall('list_machines');
}

/** Create a new machine. */
export async function createMachine(input: CreateMachineInput): Promise<Machine> {
  return apiCall('create_machine', { ...input });
}

/** Update machine display metadata. */
export async function updateMachine(id: string, input: UpdateMachineInput): Promise<Machine> {
  return apiCall('update_machine', { id, ...input });
}

/** Delete a machine by id. */
export async function deleteMachine(id: string): Promise<void> {
  return apiCall('delete_machine', { id });
}

/** Get the machine id this install is registered as. */
export async function getLocalMachineId(): Promise<string | null> {
  return apiCall('get_local_machine_id');
}

/** Set or clear the machine id for this install. */
export async function setLocalMachineId(machineId: string | null): Promise<void> {
  return apiCall('set_local_machine_id', { machineId });
}

/** OS hostname hint for first-time machine registration. */
export async function getHostname(): Promise<string> {
  return apiCall('get_hostname');
}
