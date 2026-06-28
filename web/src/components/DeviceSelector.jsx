import React from 'react';

function DeviceSelector({ devices, selectedDevice, onChange }) {
  if (devices.length === 0) return null;

  return (
    <div className="filter-group">
      <label>Device:</label>
      <select 
        value={selectedDevice} 
        onChange={(e) => onChange(e.target.value)}
        className="filter-select"
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
