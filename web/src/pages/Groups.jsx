import { useState, useEffect } from 'react';
import { api } from '../api';

export default function Groups() {
  const [groups, setGroups] = useState({});
  const [loading, setLoading] = useState(true);
  const [newName, setNewName] = useState('');
  const [newGroup, setNewGroup] = useState('');
  const [identifiers, setIdentifiers] = useState([]);

  const fetchGroups = () => {
    api.getGroups()
      .then(res => {
        setGroups(res);
        setLoading(false);
      })
      .catch(console.error);
  };

  const fetchIdentifiers = () => {
    api.getIdentifiers()
      .then(res => {
        setIdentifiers(res.identifiers || []);
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
    
    const identifier = identifiers.find(id => id.name === newName.trim());
    const identifierType = identifier ? identifier.type : 'app';
    
    api.setGroup(newName.trim(), identifierType, newGroup.trim())
      .then(() => {
        setNewName('');
        setNewGroup('');
        fetchGroups();
      })
      .catch(console.error);
  };

  const handleDelete = (name) => {
    if (!window.confirm(`Are you sure you want to remove ${name} from its group?`)) return;
    api.deleteGroup(name)
      .then(fetchGroups)
      .catch(console.error);
  };

  if (loading) return <div className="page-header"><h1 className="page-title">Loading...</h1></div>;

  const existingGroups = Array.from(new Set(Object.values(groups)));

  return (
    <>
      <div className="page-header">
        <div>
          <h1 className="page-title">Groups</h1>
          <p className="page-subtitle">Group multiple apps/domains into a single entity</p>
        </div>
      </div>

      <div className="page-body">
        <div className="grid-12">
          {/* Add Grouping Form */}
          <div className="col-span-4 glass-panel flex-col" style={{ alignSelf: 'start', padding: '24px' }}>
            <h2 className="card-title">Add Grouping</h2>
            <form onSubmit={handleAdd} className="flex-col gap-stack-md mt-4">
              <div>
                <label className="form-label">APP CLASS OR DOMAIN</label>
                <input 
                  type="text" 
                  className="input-dark" 
                  list="group-identifier-list"
                  placeholder="e.g. google.com" 
                  value={newName}
                  onChange={e => setNewName(e.target.value)}
                />
                <datalist id="group-identifier-list">
                  {identifiers.map(id => (
                    <option key={id.name} value={id.name}>{id.type}</option>
                  ))}
                </datalist>
              </div>
              <div>
                <label className="form-label">GROUP NAME</label>
                <input 
                  type="text" 
                  className="input-dark" 
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
              <button type="submit" className="btn-primary w-full mt-4">
                <span className="material-symbols-outlined" style={{ fontSize: '18px' }}>add</span>
                ADD TO GROUP
              </button>
            </form>
          </div>

          {/* Current Groups Table */}
          <div className="col-span-8 glass-panel flex-col" style={{ overflow: 'hidden' }}>
            <div style={{ padding: '24px 24px 8px 24px' }}>
              <h2 className="card-title" style={{ marginBottom: 0 }}>Current Groups</h2>
            </div>
            <div className="table-container" style={{ flex: 1, overflowY: 'auto', maxHeight: '600px' }}>
              <table className="table">
                <thead>
                  <tr>
                    <th className="bg-dark">TARGET (APP/DOMAIN)</th>
                    <th className="bg-dark">GROUP NAME</th>
                    <th className="bg-dark" style={{ width: '80px', textAlign: 'center' }}>ACTIONS</th>
                  </tr>
                </thead>
                <tbody>
                  {Object.entries(groups).map(([name, group]) => (
                    <tr key={name}>
                      <td style={{ fontWeight: 500, color: 'var(--on-surface)' }}>{name}</td>
                      <td>
                        <span className="badge" style={{ background: 'rgba(99, 102, 241, 0.1)', color: '#818cf8', border: '1px solid rgba(99, 102, 241, 0.2)' }}>
                          {group}
                        </span>
                      </td>
                      <td style={{ textAlign: 'center' }}>
                        <button 
                          className="icon-btn"
                          onClick={() => handleDelete(name)}
                          title="Remove Group"
                          style={{ margin: '0 auto' }}
                        >
                          <span className="material-symbols-outlined" style={{ fontSize: '20px' }}>delete</span>
                        </button>
                      </td>
                    </tr>
                  ))}
                  {Object.keys(groups).length === 0 && (
                    <tr>
                      <td colSpan="3" style={{ textAlign: 'center', color: 'var(--on-surface-variant)', padding: '24px' }}>
                        No groups defined.
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
