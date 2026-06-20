import { useState, useEffect } from 'react';
import axios from 'axios';
import { Trash2 } from 'lucide-react';

export default function Groups() {
  const [groups, setGroups] = useState({});
  const [loading, setLoading] = useState(true);
  const [newName, setNewName] = useState('');
  const [newGroup, setNewGroup] = useState('');
  const [identifiers, setIdentifiers] = useState([]);

  const fetchGroups = () => {
    axios.get('/api/groups')
      .then(res => {
        setGroups(res.data);
        setLoading(false);
      })
      .catch(console.error);
  };

  const fetchIdentifiers = () => {
    axios.get('/api/identifiers')
      .then(res => {
        setIdentifiers(res.data.identifiers || []);
      })
      .catch(console.error);
  };

  useEffect(() => {
    fetchGroups();
    fetchIdentifiers();
  }, []);

  const handleAdd = (e) => {
    e.preventDefault();
    if (!newName || !newGroup) return;
    axios.post('/api/groups', { name: newName.trim(), group: newGroup.trim() })
      .then(() => {
        setNewName('');
        setNewGroup('');
        fetchGroups();
      })
      .catch(console.error);
  };

  const handleDelete = (name) => {
    axios.delete('/api/groups', { data: { name } })
      .then(fetchGroups)
      .catch(console.error);
  };

  if (loading) return <div className="page-header"><h1 className="page-title">Loading...</h1></div>;

  // Get unique existing groups for autocomplete
  const existingGroups = Array.from(new Set(Object.values(groups)));

  return (
    <div>
      <div className="page-header">
        <h1 className="page-title">Groups</h1>
        <p className="page-subtitle">Group multiple apps/domains into a single entity</p>
      </div>

      <div className="grid-cards" style={{ gridTemplateColumns: '1fr 2fr' }}>
        <div className="card" style={{ alignSelf: 'start' }}>
          <h2 style={{ marginBottom: '1.5rem', fontSize: '1.2rem' }}>Add Grouping</h2>
          <form onSubmit={handleAdd}>
            <div className="form-group">
              <label className="form-label">App Class or Domain</label>
              <input 
                type="text" 
                className="input" 
                list="identifier-list"
                placeholder="e.g. google.com" 
                value={newName}
                onChange={e => setNewName(e.target.value)}
              />
              <datalist id="identifier-list">
                {identifiers.map(id => (
                  <option key={id.name} value={id.name}>{id.type}</option>
                ))}
              </datalist>
            </div>
            <div className="form-group">
              <label className="form-label">Group Name</label>
              <input 
                type="text" 
                className="input" 
                list="group-list"
                placeholder="e.g. Google Services" 
                value={newGroup}
                onChange={e => setNewGroup(e.target.value)}
              />
              <datalist id="group-list">
                {existingGroups.map(grp => (
                  <option key={grp} value={grp} />
                ))}
              </datalist>
            </div>
            <button type="submit" className="btn btn-primary" style={{ width: '100%' }}>Add to Group</button>
          </form>
        </div>

        <div className="card">
          <h2 style={{ marginBottom: '1.5rem', fontSize: '1.2rem' }}>Current Groups</h2>
          <div className="table-container">
            <table className="table">
              <thead>
                <tr>
                  <th>Target (App/Domain)</th>
                  <th>Group Name</th>
                  <th style={{ width: '80px' }}>Actions</th>
                </tr>
              </thead>
              <tbody>
                {Object.entries(groups).map(([name, group]) => (
                  <tr key={name}>
                    <td style={{ fontWeight: 500 }}>{name}</td>
                    <td><span className="badge badge-primary" style={{ background: 'rgba(6, 182, 212, 0.15)', color: '#67e8f9', borderColor: 'rgba(6, 182, 212, 0.3)' }}>{group}</span></td>
                    <td>
                      <button 
                        className="btn btn-danger" 
                        style={{ padding: '0.4rem 0.6rem' }}
                        onClick={() => handleDelete(name)}
                      >
                        <Trash2 size={16} />
                      </button>
                    </td>
                  </tr>
                ))}
                {Object.keys(groups).length === 0 && (
                  <tr>
                    <td colSpan="3" style={{ textAlign: 'center', color: 'var(--text-muted)' }}>No groups defined.</td>
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
