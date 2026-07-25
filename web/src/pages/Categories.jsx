import { useState, useEffect } from 'react';
import { api } from '../api';

function getCategoryBadgeClass(category) {
  const cat = (category || '').toLowerCase();
  if (cat.includes('social')) return 'social';
  if (cat.includes('entert')) return 'entertainment';
  if (cat.includes('llm') || cat.includes('ai')) return 'llm';
  if (cat.includes('dev')) return 'development';
  if (cat.includes('util')) return 'utilities';
  return 'uncategorized';
}

export default function Categories() {
  const [categories, setCategories] = useState({});
  const [loading, setLoading] = useState(true);
  const [newName, setNewName] = useState('');
  const [newCat, setNewCat] = useState('');
  const [identifiers, setIdentifiers] = useState([]);

  const fetchCategories = () => {
    api.getCategories()
      .then(res => {
        setCategories(res);
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
    fetchCategories();
    fetchIdentifiers();
  }, []);

  const handleAdd = (e) => {
    e.preventDefault();
    if (!newName || !newCat) return;
    
    const identifier = identifiers.find(id => id.name === newName.trim());
    const identifierType = identifier ? identifier.type : 'app';
    
    api.setCategory(newName.trim(), identifierType, newCat.trim())
      .then(() => {
        setNewName('');
        setNewCat('');
        fetchCategories();
      })
      .catch(console.error);
  };

  const handleDelete = (name) => {
    if (!window.confirm(`Are you sure you want to delete the mapping for ${name}?`)) return;
    api.deleteCategory(name)
      .then(fetchCategories)
      .catch(console.error);
  };

  if (loading) return <div className="page-header"><h1 className="page-title">Loading...</h1></div>;

  const existingCategories = Array.from(new Set(Object.values(categories)));

  return (
    <>
      <div className="page-header">
        <div>
          <h1 className="page-title">Categories</h1>
          <p className="page-subtitle">Map apps and websites to categories (e.g. youtube.com → Entertainment)</p>
        </div>
      </div>

      <div className="page-body">
        <div className="grid-12">
          {/* Add Mapping Form */}
          <div className="col-span-4 glass-panel flex-col" style={{ alignSelf: 'start', padding: '24px' }}>
            <h2 className="card-title">Add Mapping</h2>
            <form onSubmit={handleAdd} className="flex-col gap-stack-md mt-4">
              <div>
                <label className="form-label">APP CLASS OR DOMAIN</label>
                <input 
                  type="text" 
                  className="input-dark" 
                  list="category-identifier-list"
                  placeholder="e.g. youtube.com or kitty" 
                  value={newName}
                  onChange={e => setNewName(e.target.value)}
                />
                <datalist id="category-identifier-list">
                  {identifiers.map(id => (
                    <option key={id.name} value={id.name}>{id.type}</option>
                  ))}
                </datalist>
              </div>
              <div>
                <label className="form-label">CATEGORY</label>
                <input 
                  type="text" 
                  className="input-dark" 
                  list="category-list"
                  placeholder="e.g. Entertainment" 
                  value={newCat}
                  onChange={e => setNewCat(e.target.value)}
                />
                <datalist id="category-list">
                  {existingCategories.map(cat => (
                    <option key={cat} value={cat} />
                  ))}
                </datalist>
              </div>
              <button type="submit" className="btn-primary w-full mt-4">
                <span className="material-symbols-outlined" style={{ fontSize: '18px' }}>add</span>
                ADD CATEGORY
              </button>
            </form>
          </div>

          {/* Current Mappings Table */}
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
                          onClick={() => handleDelete(name)}
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
        </div>
      </div>
    </>
  );
}
