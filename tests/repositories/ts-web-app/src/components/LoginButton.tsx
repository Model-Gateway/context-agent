import { useAuth } from "../hooks/useAuth";
import "../styles/button.css";

/**
 * Primary call-to-action in the header.
 *
 * TODO(ux): there is no loading state. While `signIn()` is in flight the button
 * should be disabled and show the shared <Spinner /> from ./Spinner. `useAuth`
 * already exposes `isLoading` for exactly this purpose.
 */
export function LoginButton() {
  const { user, signIn, signOut } = useAuth();

  if (user) {
    return (
      <button type="button" className="btn" onClick={signOut}>
        Sign out ({user.name})
      </button>
    );
  }

  return (
    <button type="button" className="btn" onClick={() => signIn()}>
      Sign in
    </button>
  );
}
