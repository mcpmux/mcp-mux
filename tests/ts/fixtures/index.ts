/**
 * Test fixtures for TypeScript tests
 */

export interface Space {
  id: string;
  name: string;
  icon: string | null;
  description: string | null;
  is_default: boolean;
  sort_order: number;
  created_at: string;
  updated_at: string;
}

export interface Server {
  id: string;
  space_id: string;
  server_id: string;
  name: string;
  enabled: boolean;
  status: 'disconnected' | 'connecting' | 'connected' | 'error';
}

/**
 * Create a test space
 */
export function createTestSpace(overrides: Partial<Space> = {}): Space {
  const now = new Date().toISOString();
  return {
    id: `space-${Math.random().toString(36).substr(2, 9)}`,
    name: 'Test Space',
    icon: '🧪',
    description: 'A test space',
    is_default: false,
    sort_order: 0,
    created_at: now,
    updated_at: now,
    ...overrides,
  };
}

/**
 * Create a default test space
 */
export function createDefaultSpace(overrides: Partial<Space> = {}): Space {
  return createTestSpace({
    name: 'Default Space',
    is_default: true,
    ...overrides,
  });
}

/**
 * Create a test server
 */
export function createTestServer(spaceId: string, overrides: Partial<Server> = {}): Server {
  return {
    id: `server-${Math.random().toString(36).substr(2, 9)}`,
    space_id: spaceId,
    server_id: `def-${Math.random().toString(36).substr(2, 9)}`,
    name: 'Test Server',
    enabled: true,
    status: 'disconnected',
    ...overrides,
  };
}

/**
 * Create multiple test spaces
 */
export function createTestSpaces(count: number): Space[] {
  return Array.from({ length: count }, (_, i) =>
    createTestSpace({
      name: `Space ${i + 1}`,
      is_default: i === 0,
    })
  );
}
