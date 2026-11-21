import { useState } from 'react';

export default function CategoryForm({ identifiers, existingCategories, onAdd }) {
  const [newName, setNewName] = useState('');
  const [newCat, setNewCat] = useState('');

  const handleSubmit = (e) => {
    e.preventDefault();
    if (!newName || !newCat) return;

    onAdd(newName, newCat)
      .then(() => {
        setNewName('');
        setNewCat('');
      })
      .catch(() => {
        // Form inputs are not cleared if there's an error
      });
  };

  return (
    <div className="col-span-4 glass-panel flex-col" style={{ alignSelf: 'start', padding: '24px' }}>
      <h2 className="card-title">Add Mapping</h2>
      <form onSubmit={handleSubmit} className="flex-col gap-stack-md mt-4">
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
  );
}
