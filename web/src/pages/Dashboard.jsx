import { useState, useEffect } from 'react';
import axios from 'axios';
import { Chart as ChartJS, ArcElement, Tooltip, Legend } from 'chart.js';
import { Doughnut } from 'react-chartjs-2';

ChartJS.register(ArcElement, Tooltip, Legend);

function formatTime(seconds) {
  if (!seconds) return '0m';
  const h = Math.floor(seconds / 3600);
  const m = Math.floor((seconds % 3600) / 60);
  if (h > 0) return `${h}h ${m}m`;
  return `${m}m`;
}

export default function Dashboard() {
  const [data, setData] = useState(null);
  const [loading, setLoading] = useState(true);
  const [device, setDevice] = useState(''); // '' means all devices
  const [availableDevices, setAvailableDevices] = useState([]);

  useEffect(() => {
    // Fetch available devices
    axios.get('/api/devices')
      .then(res => {
        setAvailableDevices(res.data.devices || []);
      })
      .catch(err => console.error("Error fetching devices:", err));
  }, []);

  useEffect(() => {
    let url = '/api/summary';
    if (device) url += `?device_id=${device}`;
    setLoading(true);
    axios.get(url)
      .then(res => {
        setData(res.data);
        setLoading(false);
      })
      .catch(err => console.error(err));
  }, [device]);

  if (loading && !data) return <div className="page-header"><h1 className="page-title">Loading...</h1></div>;

  const stats = data ? (data.stats || []) : [];
  
  // Aggregate by category for chart
  const categoryTotals = {};
  stats.forEach(item => {
    const cat = item.category || 'Uncategorized';
    categoryTotals[cat] = (categoryTotals[cat] || 0) + item.total_seconds;
  });

  const chartData = {
    labels: Object.keys(categoryTotals),
    datasets: [
      {
        data: Object.values(categoryTotals).map(v => Math.round(v / 60)), // minutes
        backgroundColor: [
          '#7c3aed', '#06b6d4', '#10b981', '#f59e0b', '#ef4444', '#8b5cf6', '#3b82f6'
        ],
        borderWidth: 0,
        hoverOffset: 10,
      },
    ],
  };

  const chartOptions = {
    cutout: '75%',
    plugins: {
      legend: { position: 'right', labels: { color: '#f8fafc', font: { family: 'Inter' } } },
      tooltip: {
        callbacks: {
          label: (context) => ` ${context.label}: ${formatTime(context.raw * 60)}`
        }
      }
    }
  };

  return (
    <div>
      <div className="page-header" style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'flex-start' }}>
        <div>
          <h1 className="page-title">Today's Overview</h1>
          <p className="page-subtitle">Your screen time for {data ? new Date(data.start_ts * 1000).toLocaleDateString() : ''}</p>
        </div>
        <div>
          <select 
            value={device} 
            onChange={(e) => setDevice(e.target.value)}
            style={{ 
              padding: '0.5rem 1rem', 
              borderRadius: '8px', 
              background: '#1e293b', 
              color: 'white',
              border: '1px solid #334155',
              fontFamily: 'Inter',
              cursor: 'pointer'
            }}
          >
            <option value="">All Devices</option>
            {availableDevices.map(d => (
              <option key={d} value={d}>
                {d === 'hyprland-pc' ? 'PC (Hyprland)' : 
                 d === 'android-phone' ? 'Mobile (Android)' : d}
              </option>
            ))}
          </select>
        </div>
      </div>

      <div className="grid-cards">
        <div className="card stat-card">
          <div className="stat-title">Total Active Time</div>
          <div className="stat-value">{formatTime(data.total_seconds)}</div>
        </div>
        <div className="card stat-card">
          <div className="stat-title">Total Apps/Sites</div>
          <div className="stat-value">{stats.length}</div>
        </div>
      </div>

      <div className="grid-cards" style={{ gridTemplateColumns: '1fr 2fr' }}>
        <div className="card">
          <h2 style={{ marginBottom: '1rem', fontSize: '1.2rem' }}>Category Breakdown</h2>
          <div className="chart-container" style={{ height: '280px' }}>
            <Doughnut data={chartData} options={chartOptions} />
          </div>
        </div>

        <div className="card">
          <h2 style={{ marginBottom: '1rem', fontSize: '1.2rem' }}>Top Usage</h2>
          <div className="table-container">
            <table className="table">
              <thead>
                <tr>
                  <th>App / Website</th>
                  <th>Category</th>
                  <th>Time</th>
                </tr>
              </thead>
              <tbody>
                {stats.slice(0, 8).map((item, i) => (
                  <tr key={i}>
                    <td>
                      <div style={{ fontWeight: 500 }}>{item.name}</div>
                      {item.identifier_type === 'group' && (
                        <div style={{ fontSize: '0.8rem', color: 'var(--text-muted)' }}>
                          {item.children.length} items grouped
                        </div>
                      )}
                    </td>
                    <td>
                      <span className="badge badge-primary">{item.category || 'Uncategorized'}</span>
                    </td>
                    <td style={{ fontWeight: 600 }}>{formatTime(item.total_seconds)}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </div>
      </div>
    </div>
  );
}
