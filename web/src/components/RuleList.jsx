import RuleItem from './RuleItem';

export default function RuleList({ rules, onDelete }) {
  return (
    <div className="col-span-8 glass-panel flex-col" style={{ overflow: 'hidden' }}>
      <div style={{ padding: '24px 24px 8px 24px' }}>
        <h2 className="card-title" style={{ marginBottom: 0 }}>Active Rules</h2>
      </div>
      <div className="table-container" style={{ flex: 1, overflowY: 'auto', maxHeight: '600px' }}>
        <table className="table">
          <thead>
            <tr>
              <th className="bg-dark">TERMINAL APP</th>
              <th className="bg-dark">EXTRACTED TITLE PREFIX</th>
              <th className="bg-dark" style={{ width: '80px', textAlign: 'center' }}>ACTIONS</th>
            </tr>
          </thead>
          <tbody>
            {rules.map((rule, i) => (
              <RuleItem key={i} rule={rule} onDelete={onDelete} />
            ))}
            {rules.length === 0 && (
              <tr>
                <td colSpan="3" style={{ textAlign: 'center', color: 'var(--on-surface-variant)', padding: '24px' }}>
                  No title rules defined.
                </td>
              </tr>
            )}
          </tbody>
        </table>
      </div>
    </div>
  );
}
