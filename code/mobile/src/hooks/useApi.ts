// Returns an API client bound to the current server URL + token.

import { useMemo } from 'react';

import { createApi, type Api } from '../api/client';
import { useAuth } from '../auth/AuthContext';

export function useApi(): Api {
  const { serverUrl, token } = useAuth();
  return useMemo(() => createApi(serverUrl ?? '', token), [serverUrl, token]);
}
