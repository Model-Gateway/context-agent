import { useCallback, useState } from "react";
import { apiClient } from "../api/client";

export interface User {
  id: string;
  name: string;
  email: string;
}

export interface AuthState {
  user: User | null;
  /** True while a sign-in or sign-out request is in flight. */
  isLoading: boolean;
  error: string | null;
  signIn: () => Promise<void>;
  signOut: () => Promise<void>;
}

/**
 * Authentication hook. Wraps the `/auth/*` endpoints of the API client and
 * tracks request state so components can render pending and error states.
 */
export function useAuth(): AuthState {
  const [user, setUser] = useState<User | null>(null);
  const [isLoading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const signIn = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const nextUser = await apiClient.post<User>("/auth/login", {});
      setUser(nextUser);
    } catch (err) {
      setError(err instanceof Error ? err.message : "Sign-in failed");
    } finally {
      setLoading(false);
    }
  }, []);

  const signOut = useCallback(async () => {
    setLoading(true);
    try {
      await apiClient.post<void>("/auth/logout", {});
      setUser(null);
    } finally {
      setLoading(false);
    }
  }, []);

  return { user, isLoading, error, signIn, signOut };
}
