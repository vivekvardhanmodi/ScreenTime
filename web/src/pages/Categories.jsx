import { useState, useEffect } from 'react';
import axios from 'axios';
import { Trash2 } from 'lucide-react';

export default function Categories() {
  const [categories, setCategories] = useState({});
  const [loading, setLoading] = useState(true);
  const [newName, setNewName] = useState('');
  const [newCat, setNewCat] = useState('');
  const [identifiers, setIdentifiers] = useState([]);

  const fetchCategories = () => {
    axios.get('/api/categories')
      .then(res => {
        setCategories(res.data);
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
    fetchCategories();
    fetchIdentifiers();
  }, []);

  const handleAdd = (e) => {
    e.preventDefault();
    if (!newName || !newCat) return;
    axios.post('/api/categories', { name: newName.trim(), category: newCat.trim() })
      .then(() => {
        setNewName('');
        setNewCat('');
        fetchCategories();
      })
      .catch(console.error);
  };

  const handleDelete = (name) => {
    axios.delete('/api/categories', { data: { name } })
      .then(fetchCategories)
      .catch(console.error);
  };

  if (loading) return <div className="page-header"><h1 className="page-title">Loading...</h1></div>;

  // Get unique existing categories for autocomplete
  const existingCategories = Array.from(new Set(Object.values(categories)));

  return (
    <div>
      <div className="page-header">
        <h1 className="page-title">Categories</h1>
        <p className="page-subtitle">Map apps and websites to categories (e.g. youtube.com → Entertainment)</p>
      </div>

      <div className="grid-cards" style={{ gridTemplateColumns: '1fr 2fr' }}>
        <div className="card" style={{ alignSelf: 'start' }}>
          <h2 style={{ marginBottom: '1.5rem', fontSize: '1.2rem' }}>Add Mapping</h2>
          <form onSubmit={handleAdd}>
            <div className="form-group">
              <label className="form-label">App Class or Domain</label>
              <input 
                type="text" 
                className="input" 
                list="identifier-list"
                placeholder="e.g. youtube.com or kitty" 
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
              <label className="form-label">Category</label>
              <input 
                type="text" 
                className="input" 
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
            <button type="submit" className="btn btn-primary" style={{ width: '100%' }}>Add Category</button>
          </form>
        </div>

        <div className="card">
          <h2 style={{ marginBottom: '1.5rem', fontSize: '1.2rem' }}>Current Mappings</h2>
          <div className="table-container">
            <table className="table">
              <thead>
                <tr>
                  <th>Target (App/Domain)</th>
                  <th>Category</th>
                  <th style={{ width: '80px' }}>Actions</th>
                </tr>
              </thead>
              <tbody>
                {Object.entries(categories).map(([name, cat]) => (
                  <tr key={name}>
                    <td style={{ fontWeight: 500 }}>{name}</td>
                    <td><span className="badge badge-primary">{cat}</span></td>
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
                {Object.keys(categories).length === 0 && (
                  <tr>
                    <td colSpan="3" style={{ textAlign: 'center', color: 'var(--text-muted)' }}>No categories defined.</td>
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
