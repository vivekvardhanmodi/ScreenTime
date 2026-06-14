import { useState, useEffect } from 'react';
import axios from 'axios';
import { Trash2 } from 'lucide-react';

export default function Rules() {
  const [rules, setRules] = useState([]);
  const [loading, setLoading] = useState(true);
  const [appClass, setAppClass] = useState('');
  const [targetTitle, setTargetTitle] = useState('');

  const fetchRules = () => {
    axios.get('/api/rules')
      .then(res => {
        setRules(res.data);
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
    axios.post('/api/rules', { app_class: appClass.trim(), target_title: targetTitle.trim() })
      .then(() => {
        setAppClass('');
        setTargetTitle('');
        fetchRules();
      })
      .catch(console.error);
  };

  const handleDelete = (cls, title) => {
    axios.delete('/api/rules', { data: { app_class: cls, target_title: title } })
      .then(fetchRules)
      .catch(console.error);
  };

  if (loading) return <div className="page-header"><h1 className="page-title">Loading...</h1></div>;

  return (
    <div>
      <div className="page-header">
        <h1 className="page-title">Title Rules</h1>
        <p className="page-subtitle">Extract specific apps running inside terminal emulators</p>
      </div>

      <div className="grid-cards" style={{ gridTemplateColumns: '1fr 2fr' }}>
        <div className="card" style={{ alignSelf: 'start' }}>
          <h2 style={{ marginBottom: '1.5rem', fontSize: '1.2rem' }}>Add Rule</h2>
          <form onSubmit={handleAdd}>
            <div className="form-group">
              <label className="form-label">Terminal App Class</label>
              <input 
                type="text" 
                className="input" 
                placeholder="e.g. kitty" 
                value={appClass}
                onChange={e => setAppClass(e.target.value)}
              />
            </div>
            <div className="form-group">
              <label className="form-label">Target Title (Prefix)</label>
              <input 
                type="text" 
                className="input" 
                placeholder="e.g. nvim" 
                value={targetTitle}
                onChange={e => setTargetTitle(e.target.value)}
              />
            </div>
            <button type="submit" className="btn btn-primary" style={{ width: '100%' }}>Add Rule</button>
          </form>
        </div>

        <div className="card">
          <h2 style={{ marginBottom: '1.5rem', fontSize: '1.2rem' }}>Active Rules</h2>
          <div className="table-container">
            <table className="table">
              <thead>
                <tr>
                  <th>Terminal App</th>
                  <th>Extracted Title Prefix</th>
                  <th style={{ width: '80px' }}>Actions</th>
                </tr>
              </thead>
              <tbody>
                {rules.map((rule, i) => (
                  <tr key={i}>
                    <td style={{ fontWeight: 500 }}>{rule.app_class}</td>
                    <td><span className="badge badge-primary" style={{ background: 'rgba(16, 185, 129, 0.15)', color: '#34d399', borderColor: 'rgba(16, 185, 129, 0.3)' }}>{rule.target_title}</span></td>
                    <td>
                      <button 
                        className="btn btn-danger" 
                        style={{ padding: '0.4rem 0.6rem' }}
                        onClick={() => handleDelete(rule.app_class, rule.target_title)}
                      >
                        <Trash2 size={16} />
                      </button>
                    </td>
                  </tr>
                ))}
                {rules.length === 0 && (
                  <tr>
                    <td colSpan="3" style={{ textAlign: 'center', color: 'var(--text-muted)' }}>No title rules defined.</td>
                  </tr>
                )}
              </tbody>
            </table>
          </div>
        </div>
      </div>
    </div>
  );
}
