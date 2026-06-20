import { useState, useEffect } from 'react';
import axios from 'axios';
import { Chart as ChartJS, CategoryScale, LinearScale, BarElement, Title, Tooltip, Legend } from 'chart.js';
import { Bar } from 'react-chartjs-2';

ChartJS.register(CategoryScale, LinearScale, BarElement, Title, Tooltip, Legend);

function formatTime(seconds) {
  if (!seconds) return '0m';
  const h = Math.floor(seconds / 3600);
  const m = Math.floor((seconds % 3600) / 60);
  if (h > 0) return `${h}h ${m}m`;
  return `${m}m`;
}

export default function HistoryPage() {
  const [data, setData] = useState(null);
  const [loading, setLoading] = useState(true);
  
  // Default to last 7 days
  const [startDate, setStartDate] = useState(() => {
    const d = new Date();
    d.setDate(d.getDate() - 6);
    return d.toISOString().split('T')[0];
  });
  const [endDate, setEndDate] = useState(() => {
    return new Date().toISOString().split('T')[0];
  });

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

  const fetchData = () => {
    setLoading(true);
    let url = `/api/summary?start_date=${startDate}&end_date=${endDate}`;
    if (device) url += `&device_id=${device}`;
    axios.get(url)
      .then(res => {
        setData(res.data);
        setLoading(false);
      })
      .catch(err => console.error(err));
  };

  useEffect(() => {
    fetchData();
  }, [startDate, endDate, device]);

  if (loading && !data) return <div className="page-header"><h1 className="page-title">Loading...</h1></div>;

  const stats = data?.stats || [];
  
  const chartData = {
    labels: stats.slice(0, 10).map(s => s.name),
    datasets: [
      {
        label: 'Hours',
        data: stats.slice(0, 10).map(s => (s.total_seconds / 3600).toFixed(2)),
        backgroundColor: 'rgba(124, 58, 237, 0.8)',
        borderRadius: 6,
      },
    ],
  };

  const chartOptions = {
    responsive: true,
    maintainAspectRatio: false,
    plugins: {
      legend: { display: false },
      tooltip: {
        callbacks: {
          label: (context) => ` ${formatTime(context.raw * 3600)}`
        }
      }
    },
    scales: {
      y: {
        grid: { color: 'rgba(255, 255, 255, 0.05)' },
        ticks: { color: '#94a3b8' }
      },
      x: {
        grid: { display: false },
        ticks: { color: '#94a3b8' }
      }
    }
  };

  return (
    <div>
      <div className="page-header" style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'flex-end' }}>
        <div>
          <h1 className="page-title">History</h1>
          <p className="page-subtitle">Analyze your past activity</p>
        </div>
        <div style={{ display: 'flex', gap: '1rem', alignItems: 'flex-end' }}>
          <div className="form-group" style={{ marginBottom: 0 }}>
            <label className="form-label">Device</label>
            <select 
              className="input" 
              value={device} 
              onChange={e => setDevice(e.target.value)}
              style={{ cursor: 'pointer' }}
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
          <div className="form-group" style={{ marginBottom: 0 }}>
            <label className="form-label">Start Date</label>
            <input type="date" className="input" value={startDate} onChange={e => setStartDate(e.target.value)} />
          </div>
          <div className="form-group" style={{ marginBottom: 0 }}>
            <label className="form-label">End Date</label>
            <input type="date" className="input" value={endDate} onChange={e => setEndDate(e.target.value)} />
          </div>
        </div>
      </div>

      <div className="card" style={{ marginBottom: '2rem' }}>
        <h2 style={{ marginBottom: '1rem', fontSize: '1.2rem' }}>Top 10 Usage ({startDate} to {endDate})</h2>
        <div className="chart-container">
          <Bar data={chartData} options={chartOptions} />
        </div>
      </div>

      <div className="card">
        <h2 style={{ marginBottom: '1rem', fontSize: '1.2rem' }}>All Activity</h2>
        <div className="table-container">
          <table className="table">
            <thead>
              <tr>
                <th>App / Website</th>
                <th>Category</th>
                <th>Total Time</th>
              </tr>
            </thead>
            <tbody>
              {stats.map((item, i) => (
                <tr key={i}>
                  <td>
                    <div style={{ fontWeight: 500 }}>{item.name}</div>
                  </td>
                  <td>
                    <span className="badge badge-primary">{item.category || 'Uncategorized'}</span>
                  </td>
                  <td style={{ fontWeight: 600 }}>{formatTime(item.total_seconds)}</td>
                </tr>
              ))}
              {stats.length === 0 && (
                <tr>
                  <td colSpan="3" style={{ textAlign: 'center', color: 'var(--text-muted)' }}>No data for this period.</td>
                </tr>
              )}
            </tbody>
          </table>
        </div>
      </div>
    </div>
  );
}
