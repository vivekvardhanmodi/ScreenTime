import React from 'react';

function DeviceSelector({ devices, selectedDevice, onChange }) {
  if (devices.length === 0) return null;

  return (
    <div className="device-selector-glass">
      <span className="device-selector-label">Device:</span>
      <select 
        value={selectedDevice} 
        onChange={(e) => onChange(e.target.value)}
        className="device-selector-select"
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
