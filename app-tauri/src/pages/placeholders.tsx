export function TransformsPage() {
  return (
    <div className="flow-page">
      <h1 className="flow-title">Transforms</h1>
      <div className="flow-hero">
        <div className="flow-hero-text">
          <h2>Say it once, shape it after</h2>
          <p>
            Rewrites like organize, shorten, or formalize will live here.
            Dictation stays verbatim until then.
          </p>
        </div>
      </div>
      <div className="flow-empty">
        Transforms are not built yet. This page holds their place.
      </div>
    </div>
  );
}

export function ScratchpadPage() {
  return (
    <div className="flow-page">
      <h1 className="flow-title">Scratchpad</h1>
      <div className="flow-hero flow-hero-dict">
        <div className="flow-hero-text">
          <h2>Think out loud somewhere safe</h2>
          <p>
            A local-first notepad for raw dictation. Talk, then keep,
            polish, or throw away.
          </p>
        </div>
      </div>
      <div className="flow-empty">
        Scratchpad is not built yet. This page holds its place.
      </div>
    </div>
  );
}
