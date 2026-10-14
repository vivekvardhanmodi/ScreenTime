function getCategoryBadgeClass(category) {
  const cat = (category || '').toLowerCase();
  if (cat.includes('social')) return 'social';
  if (cat.includes('entert')) return 'entertainment';
  if (cat.includes('llm') || cat.includes('ai')) return 'llm';
  if (cat.includes('dev')) return 'development';
  if (cat.includes('util')) return 'utilities';
  return 'uncategorized';
}

export default function CategoryList({ categories, onDelete }) {
  return (
    <div className="col-span-8 glass-panel flex-col" style={{ overflow: 'hidden' }}>
      <div style={{ padding: '24px 24px 8px 24px' }}>
        <h2 className="card-title" style={{ marginBottom: 0 }}>Current Mappings</h2>
      </div>
      <div className="table-container" style={{ flex: 1, overflowY: 'auto', maxHeight: '600px' }}>
        <table className="table">
          <thead>
            <tr>
              <th className="bg-dark">TARGET (APP/DOMAIN)</th>
              <th className="bg-dark">CATEGORY</th>
              <th className="bg-dark" style={{ width: '80px', textAlign: 'center' }}>ACTIONS</th>
            </tr>
          </thead>
          <tbody>
            {Object.entries(categories).map(([name, cat]) => (
              <tr key={name}>
                <td style={{ fontWeight: 500, color: 'var(--on-surface)' }}>{name}</td>
                <td>
                  <span className={`badge ${getCategoryBadgeClass(cat)}`}>
                    {cat}
                  </span>
                </td>
                <td style={{ textAlign: 'center' }}>
                  <button
                    className="icon-btn"
                    onClick={() => onDelete(name)}
                    title="Remove Mapping"
                    style={{ margin: '0 auto' }}
                  >
                    <span className="material-symbols-outlined" style={{ fontSize: '20px' }}>delete</span>
                  </button>
                </td>
              </tr>
            ))}
            {Object.keys(categories).length === 0 && (
              <tr>
                <td colSpan="3" style={{ textAlign: 'center', color: 'var(--on-surface-variant)', padding: '24px' }}>
                  No categories defined.
                </td>
              </tr>
            )}
          </tbody>
        </table>
      </div>
    </div>
  );
}
