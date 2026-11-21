import { useState } from 'react';

export default function RuleForm({ onAdd }) {
  const [appClass, setAppClass] = useState('');
  const [targetTitle, setTargetTitle] = useState('');

  const handleSubmit = (e) => {
    e.preventDefault();
    if (!appClass || !targetTitle) return;
    onAdd(appClass.trim(), targetTitle.trim());
    setAppClass('');
    setTargetTitle('');
  };

  return (
    <div className="col-span-4 glass-panel flex-col" style={{ alignSelf: 'start', padding: '24px' }}>
      <h2 className="card-title">Add Rule</h2>
      <form onSubmit={handleSubmit} className="flex-col gap-stack-md mt-4">
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
  );
}
