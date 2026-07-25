import { useState, useEffect } from 'react';
import { api } from '../api';

export default function Rules() {
  const [rules, setRules] = useState([]);
  const [loading, setLoading] = useState(true);
  const [appClass, setAppClass] = useState('');
  const [targetTitle, setTargetTitle] = useState('');

  const fetchRules = () => {
    api.getRules()
      .then(res => {
        setRules(res);
        setLoading(false);
      })
      .catch(console.error);
  };

  useEffect(() => {
    fetchRules();
  }, []);

  const handleAdd = (e) => {
    e.preventDefault();
    if (!appClass || !targetTitle) return;
    api.setRule(appClass.trim(), targetTitle.trim())
      .then(() => {
        setAppClass('');
        setTargetTitle('');
        fetchRules();
      })
      .catch(console.error);
  };

  const handleDelete = (cls, title) => {
    if (!window.confirm(`Are you sure you want to delete rule ${title} for ${cls}?`)) return;
    api.deleteRule(cls, title)
      .then(fetchRules)
      .catch(console.error);
  };

  if (loading) return <div className="page-header"><h1 className="page-title">Loading...</h1></div>;

  return (
    <>
      <div className="page-header">
        <div>
          <h1 className="page-title">Title Rules</h1>
          <p className="page-subtitle">Extract specific apps running inside terminal emulators</p>
        </div>
      </div>

      <div className="page-body">
        <div className="grid-12">
          {/* Add Rule Form */}
          <div className="col-span-4 glass-panel flex-col" style={{ alignSelf: 'start', padding: '24px' }}>
            <h2 className="card-title">Add Rule</h2>
            <form onSubmit={handleAdd} className="flex-col gap-stack-md mt-4">
              <div>
                <label className="form-label">TERMINAL APP CLASS</label>
                <input 
                  type="text" 
                  className="input-dark" 
                  placeholder="e.g. kitty" 
                  value={appClass}
                  onChange={e => setAppClass(e.target.value)}
                />
              </div>
              <div>
                <label className="form-label">TARGET TITLE (PREFIX)</label>
                <input 
                  type="text" 
                  className="input-dark" 
                  placeholder="e.g. nvim" 
                  value={targetTitle}
                  onChange={e => setTargetTitle(e.target.value)}
                />
              </div>
              <button type="submit" className="btn-primary w-full mt-4">
                <span className="material-symbols-outlined" style={{ fontSize: '18px' }}>add</span>
                ADD RULE
              </button>
            </form>
          </div>

          {/* Active Rules Table */}
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
                    <tr key={i}>
                      <td style={{ fontWeight: 500, color: 'var(--on-surface)' }}>{rule.app_class}</td>
                      <td>
                        <span className="badge" style={{ background: 'rgba(16, 185, 129, 0.1)', color: '#10b981', border: '1px solid rgba(16, 185, 129, 0.2)' }}>
                          {rule.target_title}
                        </span>
                      </td>
                      <td style={{ textAlign: 'center' }}>
                        <button 
                          className="icon-btn"
                          onClick={() => handleDelete(rule.app_class, rule.target_title)}
                          title="Remove Rule"
                          style={{ margin: '0 auto' }}
                        >
                          <span className="material-symbols-outlined" style={{ fontSize: '20px' }}>delete</span>
                        </button>
                      </td>
                    </tr>
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
        </div>
      </div>
    </>
  );
}
