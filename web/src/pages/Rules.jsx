import { useState, useEffect } from 'react';
import { api } from '../api';
import RuleForm from '../components/RuleForm';
import RuleList from '../components/RuleList';

export default function Rules() {
  const [rules, setRules] = useState([]);
  const [loading, setLoading] = useState(true);

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

  const handleAdd = (appClass, targetTitle) => {
    api.setRule(appClass, targetTitle)
      .then(() => {
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
          <RuleForm onAdd={handleAdd} />
          <RuleList rules={rules} onDelete={handleDelete} />
        </div>
      </div>
    </>
  );
}
