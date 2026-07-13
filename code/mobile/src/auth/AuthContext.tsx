// Authentication state: holds the backend URL + JWT, persisted in the device's
// secure storage (Keychain on iOS) via expo-secure-store.

import React, {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useState,
} from 'react';
import * as SecureStore from 'expo-secure-store';
import * as LocalAuthentication from 'expo-local-authentication';

import { ApiError, createApi } from '../api/client';

const KEY_TOKEN = 'tj_token';
const KEY_SERVER = 'tj_server_url';

interface AuthState {
  loading: boolean; // restoring persisted session on launch
  token: string | null;
  serverUrl: string | null;
  isAuthenticated: boolean;
  /** True when a persisted session needs a biometric unlock before use. */
  locked: boolean;
  signIn: (serverUrl: string, password: string) => Promise<void>;
  signOut: () => Promise<void>;
  /** Prompt Face ID / Touch ID; clears `locked` on success. */
  unlock: () => Promise<boolean>;
}

const AuthContext = createContext<AuthState | undefined>(undefined);

export function AuthProvider({ children }: { children: React.ReactNode }) {
  const [loading, setLoading] = useState(true);
  const [token, setToken] = useState<string | null>(null);
  const [serverUrl, setServerUrl] = useState<string | null>(null);
  const [locked, setLocked] = useState(false);

  // Restore a persisted session on startup; lock it behind biometrics if the
  // device supports them (Face ID / Touch ID).
  useEffect(() => {
    (async () => {
      try {
        const [savedToken, savedServer] = await Promise.all([
          SecureStore.getItemAsync(KEY_TOKEN),
          SecureStore.getItemAsync(KEY_SERVER),
        ]);
        setToken(savedToken);
        setServerUrl(savedServer);
        if (savedToken) {
          const [hasHw, enrolled] = await Promise.all([
            LocalAuthentication.hasHardwareAsync(),
            LocalAuthentication.isEnrolledAsync(),
          ]);
          if (hasHw && enrolled) setLocked(true);
        }
      } finally {
        setLoading(false);
      }
    })();
  }, []);

  const signIn = useCallback(async (url: string, password: string) => {
    const api = createApi(url);
    const res = await api.login(password); // throws ApiError on failure
    if (typeof res?.token !== 'string' || res.token.length === 0) {
      // e.g. the URL points at the wrong server and returned no token.
      throw new ApiError(0, "Réponse de login invalide — vérifie l'URL du serveur (http://IP:8080).");
    }
    await Promise.all([
      SecureStore.setItemAsync(KEY_TOKEN, res.token),
      SecureStore.setItemAsync(KEY_SERVER, api.baseUrl),
    ]);
    setServerUrl(api.baseUrl);
    setToken(res.token);
    setLocked(false); // just authenticated with the password
  }, []);

  const signOut = useCallback(async () => {
    await SecureStore.deleteItemAsync(KEY_TOKEN);
    setToken(null);
    setLocked(false);
  }, []);

  const unlock = useCallback(async () => {
    const res = await LocalAuthentication.authenticateAsync({
      promptMessage: 'Déverrouiller Trading Journal',
      fallbackLabel: 'Utiliser le code',
    });
    if (res.success) {
      setLocked(false);
      return true;
    }
    return false;
  }, []);

  const value = useMemo<AuthState>(
    () => ({
      loading,
      token,
      serverUrl,
      isAuthenticated: !!token,
      locked,
      signIn,
      signOut,
      unlock,
    }),
    [loading, token, serverUrl, locked, signIn, signOut, unlock],
  );

  return <AuthContext.Provider value={value}>{children}</AuthContext.Provider>;
}

export function useAuth(): AuthState {
  const ctx = useContext(AuthContext);
  if (!ctx) throw new Error('useAuth must be used within an AuthProvider');
  return ctx;
}
