import React from 'react';

function DeviceSelector({ devices, selectedDevice, onChange }) {
  if (devices.length === 0) return null;

  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: '4px' }}>
      <label htmlFor="device-select" className="form-label" style={{ marginBottom: 0 }}>DEVICE</label>
      <select 
        id="device-select"
        value={selectedDevice} 
        onChange={(e) => onChange(e.target.value)}
        className="input-dark"
        style={{ padding: '8px 12px', cursor: 'pointer', minWidth: '150px' }}
      >
        <option value="all">All Devices</option>
        {devices.map(d => (
          <option key={d} value={d}>{d}</option>
        ))}
      </select>
    </div>
  );
}

export default DeviceSelector;
