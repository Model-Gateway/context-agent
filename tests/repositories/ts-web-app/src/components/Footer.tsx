const COPYRIGHT_YEAR = 2025;

/**
 * Site footer rendered at the bottom of every page.
 */
export function Footer() {
  return (
    <footer className="footer">
      <small>
        &copy; {COPYRIGHT_YEAR} Acme Corporation. All rights reserved.
      </small>
      <nav aria-label="Footer">
        <a href="/privacy">Privacy</a>
        <a href="/terms">Terms</a>
        <a href="/status">Status</a>
      </nav>
    </footer>
  );
}
