import { buildQuery } from '../fetch-api.helpers';
import type { RouteHandler } from '../fetch-api.types';

/** Clients, feature sets, and OAuth grant admin routes. */
export const permissionsRoutes: Record<string, RouteHandler> = {
  list_clients: () => ({ method: 'GET', path: '/api/v1/clients' }),
  get_client: (args) => ({
    method: 'GET',
    path: `/api/v1/clients/${encodeURIComponent(String(args.id))}`,
  }),
  create_client: (args) => ({
    method: 'POST',
    path: '/api/v1/clients',
    body: args.input as Record<string, unknown>,
  }),
  delete_client: (args) => ({
    method: 'DELETE',
    path: `/api/v1/clients/${encodeURIComponent(String(args.id))}`,
  }),
  init_preset_clients: () => ({ method: 'POST', path: '/api/v1/clients/init-presets' }),
  list_feature_sets: () => ({ method: 'GET', path: '/api/v1/feature-sets' }),
  list_feature_sets_by_space: (args) => ({
    method: 'GET',
    path: `/api/v1/feature-sets/by-space/${encodeURIComponent(String(args.spaceId))}`,
  }),
  get_feature_set: (args) => ({
    method: 'GET',
    path: `/api/v1/feature-sets/${encodeURIComponent(String(args.id))}`,
  }),
  get_feature_set_with_members: (args) => ({
    method: 'GET',
    path: `/api/v1/feature-sets/${encodeURIComponent(String(args.id))}/with-members`,
  }),
  create_feature_set: (args) => ({
    method: 'POST',
    path: '/api/v1/feature-sets',
    body: args.input as Record<string, unknown>,
  }),
  update_feature_set: (args) => ({
    method: 'PUT',
    path: `/api/v1/feature-sets/${encodeURIComponent(String(args.id))}`,
    body: args.input as Record<string, unknown>,
  }),
  delete_feature_set: (args) => ({
    method: 'DELETE',
    path: `/api/v1/feature-sets/${encodeURIComponent(String(args.id))}`,
  }),
  add_feature_set_member: (args) => ({
    method: 'POST',
    path: `/api/v1/feature-sets/${encodeURIComponent(String(args.featureSetId))}/members`,
    body: args.input as Record<string, unknown>,
  }),
  remove_feature_set_member: (args) => ({
    method: 'DELETE',
    path: `/api/v1/feature-sets/${encodeURIComponent(String(args.featureSetId))}/members/${encodeURIComponent(String(args.memberId))}`,
  }),
  set_feature_set_members: (args) => ({
    method: 'PUT',
    path: `/api/v1/feature-sets/${encodeURIComponent(String(args.featureSetId))}/members`,
    body: { members: args.members },
  }),
  get_oauth_clients: () => ({ method: 'GET', path: '/api/v1/oauth/clients' }),
  get_oauth_client_grants: (args) => ({
    method: 'GET',
    path: `/api/v1/oauth/clients/${encodeURIComponent(String(args.clientId))}/grants/${encodeURIComponent(String(args.spaceId))}`,
  }),
  update_oauth_client: (args) => ({
    method: 'PUT',
    path: `/api/v1/oauth/clients/${encodeURIComponent(String(args.clientId))}`,
    body: {
      client_alias: (args.settings as { client_alias?: string } | undefined)?.client_alias,
    },
  }),
  delete_oauth_client: (args) => ({
    method: 'DELETE',
    path: `/api/v1/oauth/clients/${encodeURIComponent(String(args.clientId))}`,
  }),
  grant_oauth_client_feature_set: (args) => ({
    method: 'POST',
    path: `/api/v1/oauth/clients/${encodeURIComponent(String(args.clientId))}/grants`,
    body: { space_id: args.spaceId, feature_set_id: args.featureSetId },
  }),
  revoke_oauth_client_feature_set: (args) => ({
    method: 'POST',
    path: `/api/v1/oauth/clients/${encodeURIComponent(String(args.clientId))}/grants/revoke`,
    body: { space_id: args.spaceId, feature_set_id: args.featureSetId },
  }),
  get_pending_consent: (args) => ({
    method: 'GET',
    path: `/api/v1/oauth/consent/pending${buildQuery({ requestId: args.requestId })}`,
  }),
  approve_oauth_consent: (args) => {
    const request = args.request as
      | { request_id?: string; consent_token?: string; client_alias?: string | null }
      | undefined;
    return {
      method: 'POST',
      path: '/api/v1/oauth/consent/approve',
      body: {
        request_id: request?.request_id,
        consent_token: request?.consent_token,
        client_alias: request?.client_alias ?? null,
      },
    };
  },
  reject_oauth_consent: (args) => {
    const request = args.request as { request_id?: string; consent_token?: string } | undefined;
    return {
      method: 'POST',
      path: '/api/v1/oauth/consent/reject',
      body: {
        request_id: request?.request_id,
        consent_token: request?.consent_token,
      },
    };
  },
};
