import { useState, useEffect } from 'react';
import { Chart as ChartJS, CategoryScale, LinearScale, BarElement, Title, Tooltip, Legend } from 'chart.js';
import { Bar } from 'react-chartjs-2';
import { api } from '../api';
import { formatTime } from '../utils';
import DeviceSelector from '../components/DeviceSelector';

ChartJS.register(CategoryScale, LinearScale, BarElement, Title, Tooltip, Legend);

export default function HistoryPage() {
  const [data, setData] = useState(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState(null);
  
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
    api.getDevices()
      .then(res => setAvailableDevices(res.devices || []))
      .catch(err => console.error("Error fetching devices:", err));
  }, []);

  const fetchData = () => {
    if (startDate > endDate) {
      setError("Start Date cannot be after End Date.");
      setLoading(false);
      return;
    }
    
    setLoading(true);
    setError(null);
    api.getSummary(startDate, endDate, device)
      .then(res => {
        setData(res);
        setLoading(false);
      })
      .catch(err => {
        console.error(err);
        setError("Failed to load history data.");
        setLoading(false);
      });
  };

  useEffect(() => {
    fetchData();
  }, [startDate, endDate, device]);

  if (loading && !data) return <div className="page-header"><h1 className="page-title">Loading...</h1></div>;
  if (error) return <div className="page-header"><h1 className="page-title text-red-500">{error}</h1></div>;

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
            <DeviceSelector devices={availableDevices} selectedDevice={device} onChange={setDevice} />
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

      <div className="card" style={{ marginBottom: '2rem', display: 'flex', alignItems: 'center', gap: '1.5rem', background: 'linear-gradient(135deg, rgba(124, 58, 237, 0.1) 0%, rgba(30, 41, 59, 0.5) 100%)', border: '1px solid rgba(124, 58, 237, 0.2)' }}>
        <div style={{ flex: 1 }}>
          <h2 style={{ fontSize: '1.1rem', color: '#94a3b8', marginBottom: '0.5rem', fontWeight: 500 }}>Total Screen Time</h2>
          <div style={{ fontSize: '2.5rem', fontWeight: 700, color: '#fff', letterSpacing: '-0.02em' }}>
            {formatTime(data?.total_seconds || 0)}
          </div>
          <p style={{ color: '#64748b', fontSize: '0.9rem', marginTop: '0.25rem' }}>
            For {device === '' ? 'All Devices' : (device === 'hyprland-pc' ? 'PC (Hyprland)' : device === 'android-phone' ? 'Mobile (Android)' : device)}
            {' '}between {startDate} and {endDate}
          </p>
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
