export default function RuleItem({ rule, onDelete }) {
  return (
    <tr>
      <td style={{ fontWeight: 500, color: 'var(--on-surface)' }}>{rule.app_class}</td>
      <td>
        <span className="badge" style={{ background: 'rgba(16, 185, 129, 0.1)', color: '#10b981', border: '1px solid rgba(16, 185, 129, 0.2)' }}>
          {rule.target_title}
        </span>
      </td>
      <td style={{ textAlign: 'center' }}>
        <button
          className="icon-btn"
          onClick={() => onDelete(rule.app_class, rule.target_title)}
          title="Remove Rule"
          style={{ margin: '0 auto' }}
        >
          <span className="material-symbols-outlined" style={{ fontSize: '20px' }}>delete</span>
        </button>
      </td>
    </tr>
  );
}
