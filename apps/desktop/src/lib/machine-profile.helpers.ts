import type { Machine } from '@/lib/api/machines';

/** Editable machine profile fields shown across identity forms. */
export type MachineProfileField = 'name' | 'icon' | 'hostname';

/** Raw machine profile input before trim/validation. */
export interface MachineProfileInput {
  name: string;
  icon: string;
  hostname: string;
}

/**
 * Trim machine profile field values for validation and persistence.
 */
export function trimMachineProfile(input: MachineProfileInput): MachineProfileInput {
  return {
    name: input.name.trim(),
    icon: input.icon.trim(),
    hostname: input.hostname.trim(),
  };
}

/**
 * Return the first missing required machine profile field, if any.
 */
export function getMissingMachineProfileField(
  input: MachineProfileInput,
): MachineProfileField | null {
  const trimmed = trimMachineProfile(input);
  if (!trimmed.name) {
    return 'name';
  }
  if (!trimmed.icon) {
    return 'icon';
  }
  if (!trimmed.hostname) {
    return 'hostname';
  }
  return null;
}

/**
 * True when name, icon, and hostname are all non-empty after trim.
 */
export function isMachineProfileComplete(input: MachineProfileInput): boolean {
  return getMissingMachineProfileField(input) === null;
}

/**
 * Build the API payload for create/update machine calls.
 */
export function toMachineProfilePayload(input: MachineProfileInput): {
  name: string;
  icon: string;
  hostname: string;
} {
  return trimMachineProfile(input);
}

/**
 * i18n key under the settings namespace for a missing profile field.
 */
export function machineProfileFieldErrorKey(field: MachineProfileField): string {
  return `machineIdentity.${field}Required`;
}

/**
 * True when a persisted machine row has all required profile fields.
 */
export function isMachineRowComplete(machine: Machine): boolean {
  return isMachineProfileComplete({
    name: machine.name,
    icon: machine.icon ?? '',
    hostname: machine.hostname ?? '',
  });
}
