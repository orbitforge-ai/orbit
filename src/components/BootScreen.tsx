export function BootScreen() {
  return (
    <div className="boot-screen" role="status" aria-live="polite" aria-label="Loading Orbit">
      <div className="boot-screen__halo" aria-hidden="true" />
      <div className="boot-screen__panel">
        <div className="boot-screen__mark" aria-hidden="true">
          <span className="boot-screen__ring boot-screen__ring--outer" />
          <span className="boot-screen__ring boot-screen__ring--inner" />
          <span className="boot-screen__core" />
        </div>
        <div className="boot-screen__copy">
          <p className="boot-screen__eyebrow">Orbit</p>
          <h1 className="boot-screen__title">Loading your workspace</h1>
          <p className="boot-screen__subtitle">
            Restoring projects, sessions, and local state.
          </p>
        </div>
      </div>
    </div>
  );
}
