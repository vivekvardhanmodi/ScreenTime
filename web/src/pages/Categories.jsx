import { useState, useEffect } from 'react';
import { api } from '../api';
import CategoryForm from '../components/CategoryForm';
import CategoryList from '../components/CategoryList';

export default function Categories() {
  const [categories, setCategories] = useState({});
  const [loading, setLoading] = useState(true);
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

  const handleAdd = (newName, newCat) => {
    const identifier = identifiers.find(id => id.name === newName.trim());
    const identifierType = identifier ? identifier.type : 'app';
    
    return api.setCategory(newName.trim(), identifierType, newCat.trim())
      .then(() => {
        fetchCategories();
      })
      .catch(err => {
        console.error(err);
        throw err;
      });
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
          <CategoryForm
            identifiers={identifiers}
            existingCategories={existingCategories}
            onAdd={handleAdd}
          />
          <CategoryList
            categories={categories}
            onDelete={handleDelete}
          />
        </div>
      </div>
    </>
  );
}
