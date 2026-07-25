import { useState, useEffect } from 'react';
import { Chart as ChartJS, CategoryScale, LinearScale, BarElement, Title, Tooltip, Legend } from 'chart.js';
import { Bar } from 'react-chartjs-2';
import { api } from '../api';
import { formatTime } from '../utils';
import DeviceSelector from '../components/DeviceSelector';

ChartJS.register(CategoryScale, LinearScale, BarElement, Title, Tooltip, Legend);

function getCategoryBadgeClass(category) {
  const cat = (category || '').toLowerCase();
  if (cat.includes('social')) return 'social';
  if (cat.includes('entert')) return 'entertainment';
  if (cat.includes('llm') || cat.includes('ai')) return 'llm';
  if (cat.includes('dev')) return 'development';
  if (cat.includes('util')) return 'utilities';
  return 'uncategorized';
}

export default function HistoryPage() {
  const [data, setData] = useState(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState(null);
  
  const [startDate, setStartDate] = useState(() => {
    const d = new Date();
    d.setDate(d.getDate() - 6);
    return d.toISOString().split('T')[0];
  });
  const [endDate, setEndDate] = useState(() => {
    return new Date().toISOString().split('T')[0];
  });

  const [device, setDevice] = useState('');
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
  if (error) return <div className="page-header"><h1 className="page-title" style={{color: 'var(--error)'}}>{error}</h1></div>;

  const stats = data?.stats || [];
  
  const numDays = Math.max(1, Math.round((new Date(endDate) - new Date(startDate)) / (1000 * 60 * 60 * 24)) + 1);
  const dailyAverageSeconds = (data?.total_seconds || 0) / numDays;
  
  const chartData = {
    labels: stats.slice(0, 10).map(s => s.name),
    datasets: [
      {
        label: 'Hours',
        data: stats.slice(0, 10).map(s => (s.total_seconds / 3600).toFixed(2)),
        backgroundColor: '#8083ff',
        borderRadius: 4,
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
        grid: { color: 'rgba(255, 255, 255, 0.08)' },
        ticks: { 
          color: '#c7c4d7', 
          font: { family: 'JetBrains Mono', size: 12 } 
        }
      },
      x: {
        grid: { display: false },
        ticks: { 
          color: '#c7c4d7', 
          font: { family: 'Hanken Grotesk', size: 11 },
          maxRotation: 20,
          minRotation: 20
        }
      }
    }
  };

  return (
    <>
      <div className="page-header" style={{ alignItems: 'flex-end' }}>
        <div>
          <h1 className="page-title">History</h1>
        </div>
        <div style={{ display: 'flex', alignItems: 'flex-end', gap: '16px', flexWrap: 'wrap' }}>
          <DeviceSelector devices={availableDevices} selectedDevice={device} onChange={setDevice} />
          <div style={{ display: 'flex', flexDirection: 'column', gap: '4px' }}>
            <span className="form-label" style={{ marginBottom: 0 }}>START DATE</span>
            <div className="date-input-wrapper">
              <input type="date" value={startDate} onChange={e => setStartDate(e.target.value)} />
            </div>
          </div>
          <div style={{ display: 'flex', flexDirection: 'column', gap: '4px' }}>
            <span className="form-label" style={{ marginBottom: 0 }}>END DATE</span>
            <div className="date-input-wrapper">
              <input type="date" value={endDate} onChange={e => setEndDate(e.target.value)} />
            </div>
          </div>
        </div>
      </div>

      <div className="page-body">
        {/* Screen Time Summary Card */}
        <div className="glass-panel stat-card" style={{ padding: '24px', marginBottom: '24px' }}>
          <span className="stat-label" style={{ marginBottom: '16px', display: 'block' }}>SCREEN TIME SUMMARY</span>
          <div style={{ display: 'flex', gap: '48px', alignItems: 'flex-end', marginBottom: '16px', flexWrap: 'wrap' }}>
            <div style={{ display: 'flex', flexDirection: 'column' }}>
              <span className="form-label" style={{ marginBottom: '4px' }}>TOTAL</span>
              <div style={{ fontFamily: 'var(--font-display)', fontSize: '48px', fontWeight: 700, color: 'var(--on-surface)', lineHeight: 1 }}>
                {formatTime(data?.total_seconds || 0)}
              </div>
            </div>
            <div style={{ display: 'flex', flexDirection: 'column' }}>
              <span className="form-label" style={{ marginBottom: '4px' }}>DAILY AVERAGE</span>
              <div style={{ fontFamily: 'var(--font-display)', fontSize: '48px', fontWeight: 700, color: 'var(--primary)', lineHeight: 1 }}>
                {formatTime(dailyAverageSeconds)}
              </div>
            </div>
          </div>
          <p style={{ color: 'var(--on-surface-variant)', fontSize: '14px' }}>
            For {device === '' || device === 'all' ? 'All Devices' : (device === 'hyprland-pc' ? 'PC (Hyprland)' : device === 'android-phone' ? 'Mobile (Android)' : device)} between {startDate} and {endDate}
          </p>
        </div>

        <div className="grid-12">
          {/* Chart Section */}
          <div className="col-span-12 glass-panel flex-col" style={{ padding: '24px', marginBottom: '24px' }}>
            <h2 className="card-title">Top 10 Usage ({startDate} to {endDate})</h2>
            <div className="chart-container" style={{ height: '350px', width: '100%', marginTop: '16px' }}>
              <Bar data={chartData} options={chartOptions} />
            </div>
          </div>

          {/* Table Section */}
          <div className="col-span-12 glass-panel flex-col" style={{ overflow: 'hidden' }}>
            <div style={{ padding: '24px 24px 8px 24px' }}>
              <h2 className="card-title" style={{ marginBottom: 0 }}>All Activity</h2>
            </div>
            <div className="table-container">
              <table className="table">
                <thead>
                  <tr>
                    <th className="bg-dark">APP / WEBSITE</th>
                    <th className="bg-dark">CATEGORY</th>
                    <th className="bg-dark" style={{ textAlign: 'right' }}>TOTAL TIME</th>
                  </tr>
                </thead>
                <tbody>
                  {stats.map((item, i) => (
                    <tr key={i} className="row-lg">
                      <td>
                        <div style={{ fontWeight: 500, color: 'var(--on-surface)' }}>{item.name}</div>
                      </td>
                      <td>
                        <span className={`badge ${getCategoryBadgeClass(item.category)}`}>
                          {item.category || 'Uncategorized'}
                        </span>
                      </td>
                      <td style={{ fontWeight: 600, textAlign: 'right', color: 'var(--on-surface)' }}>
                        {formatTime(item.total_seconds)}
                      </td>
                    </tr>
                  ))}
                  {stats.length === 0 && (
                    <tr>
                      <td colSpan="3" style={{ textAlign: 'center', color: 'var(--on-surface-variant)', padding: '24px' }}>
                        No data for this period.
                      </td>
                    </tr>
                  )}
                </tbody>
              </table>
            </div>
          </div>
        </div>
      </div>
    </>
  );
}
