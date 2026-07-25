import { useState, useEffect } from 'react';
import { Chart as ChartJS, ArcElement, Tooltip, Legend } from 'chart.js';
import { Doughnut } from 'react-chartjs-2';
import { api } from '../api';
import { formatTime } from '../utils';
import DeviceSelector from '../components/DeviceSelector';

ChartJS.register(ArcElement, Tooltip, Legend);

function getCategoryBadgeClass(category) {
  const cat = (category || '').toLowerCase();
  if (cat.includes('social')) return 'social';
  if (cat.includes('entert')) return 'entertainment';
  if (cat.includes('llm') || cat.includes('ai')) return 'llm';
  if (cat.includes('dev')) return 'development';
  if (cat.includes('util')) return 'utilities';
  return 'uncategorized';
}

export default function Dashboard() {
  const [data, setData] = useState(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState(null);
  const [device, setDevice] = useState('all'); 
  const [availableDevices, setAvailableDevices] = useState([]);

  useEffect(() => {
    api.getDevices()
      .then(res => setAvailableDevices(res.devices || []))
      .catch(err => console.error("Error fetching devices:", err));
  }, []);

  useEffect(() => {
    setLoading(true);
    setError(null);
    api.getSummary(null, null, device)
      .then(res => {
        setData(res);
        setLoading(false);
      })
      .catch(err => {
        console.error(err);
        setError("Failed to load dashboard data.");
        setLoading(false);
      });
  }, [device]);

  if (loading && !data) return <div className="page-header"><h1 className="page-title">Loading...</h1></div>;
  if (error) return <div className="page-header"><h1 className="page-title" style={{color: 'var(--error)'}}>{error}</h1></div>;

  const stats = data ? (data.stats || []) : [];
  
  const categoryTotals = {};
  stats.forEach(item => {
    const cat = item.category || 'Uncategorized';
    categoryTotals[cat] = (categoryTotals[cat] || 0) + item.total_seconds;
  });

  const chartData = {
    labels: Object.keys(categoryTotals),
    datasets: [
      {
        data: Object.values(categoryTotals).map(v => Math.round(v / 60)), 
        backgroundColor: [
          '#8083ff', '#4edea3', '#10b981', '#ffb95f', '#ef4444', '#c0c1ff', '#0d0096'
        ],
        borderWidth: 0,
        hoverOffset: 10,
      },
    ],
  };

  const chartOptions = {
    cutout: '75%',
    plugins: {
      legend: { position: 'right', labels: { color: '#e5e2e3', font: { family: 'Hanken Grotesk' } } },
      tooltip: {
        callbacks: {
          label: (context) => ` ${context.label}: ${formatTime(context.raw * 60)}`
        }
      }
    }
  };

  return (
    <>
      <div className="page-header">
        <div>
          <h1 className="page-title">Today's Overview</h1>
          <p className="page-subtitle">Your screen time for {data ? new Date(data.start_ts * 1000).toLocaleDateString() : ''}</p>
        </div>
        <DeviceSelector devices={availableDevices} selectedDevice={device} onChange={setDevice} />
      </div>

      <div className="page-body">
        {/* Metric Cards Row */}
        <div className="grid-12 mb-gutter">
          <div className="col-span-6 glass-panel stat-card">
            <span className="stat-label">TOTAL ACTIVE TIME</span>
            <div className="stat-value">{formatTime(data.total_seconds)}</div>
          </div>
          <div className="col-span-6 glass-panel stat-card">
            <span className="stat-label">TOTAL APPS/SITES</span>
            <div className="stat-value">{stats.length}</div>
          </div>
        </div>

        {/* Charts & Tables Row */}
        <div className="grid-12">
          <div className="col-span-5 glass-panel flex-col" style={{ padding: '24px' }}>
            <h2 className="card-title">Category Breakdown</h2>
            <div className="chart-container" style={{ marginTop: 'auto', marginBottom: 'auto' }}>
              <Doughnut data={chartData} options={chartOptions} />
            </div>
          </div>

          <div className="col-span-7 glass-panel flex-col" style={{ overflow: 'hidden', minHeight: '500px' }}>
            <div style={{ padding: '24px 24px 8px 24px' }}>
              <h2 className="card-title" style={{ marginBottom: 0 }}>Top Usage</h2>
            </div>
            <div className="table-container" style={{ flex: 1, overflowY: 'auto' }}>
              <table className="table">
                <thead>
                  <tr>
                    <th className="bg-dark" style={{ width: '50%' }}>APP / WEBSITE</th>
                    <th className="bg-dark">CATEGORY</th>
                    <th className="bg-dark" style={{ textAlign: 'right' }}>TIME</th>
                  </tr>
                </thead>
                <tbody>
                  {stats.slice(0, 10).map((item, i) => (
                    <tr key={i} className={item.identifier_type === 'group' ? 'row-xl' : 'row-lg'}>
                      <td>
                        <div style={{ fontWeight: 500, color: 'var(--on-surface)' }}>{item.name}</div>
                        {item.identifier_type === 'group' && (
                          <div style={{ fontSize: '14px', color: 'var(--on-surface-variant)', marginTop: '4px' }}>
                            {item.children?.length || 0} items grouped
                          </div>
                        )}
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
                </tbody>
              </table>
            </div>
          </div>
        </div>
      </div>
    </>
  );
}
