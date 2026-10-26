import { LoginButton } from "./components/LoginButton";
import { Footer } from "./components/Footer";

/**
 * Root of the application. Keep this file thin: layout only, no data access.
 */
export function App() {
  return (
    <main className="app">
      <header className="app-header">
        <h1>Acme Dashboard</h1>
        <LoginButton />
      </header>
      <section className="app-body">
        <p>Welcome back. Pick a project from the sidebar to get started.</p>
      </section>
      <Footer />
    </main>
  );
}
