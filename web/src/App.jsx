import { BrowserRouter, Routes, Route, NavLink } from 'react-router-dom';
import Dashboard from './pages/Dashboard';
import HistoryPage from './pages/HistoryPage';
import Categories from './pages/Categories';
import Groups from './pages/Groups';
import Rules from './pages/Rules';

function App() {
  return (
    <BrowserRouter>
      <div className="app-container">
        {/* Sidebar */}
        <nav className="sidebar">
          <div className="sidebar-header">
            <span className="material-symbols-outlined sidebar-logo-icon" style={{ fontVariationSettings: '"FILL" 1' }}>dataset</span>
            <span className="sidebar-title">ScreenTime</span>
          </div>
          <div className="nav-links">
            <NavLink to="/" className={({isActive}) => `nav-link ${isActive ? 'active' : ''}`}>
              <span className="material-symbols-outlined">dashboard</span> Dashboard
            </NavLink>
            <NavLink to="/history" className={({isActive}) => `nav-link ${isActive ? 'active' : ''}`}>
              <span className="material-symbols-outlined">history</span> History
            </NavLink>
            <NavLink to="/categories" className={({isActive}) => `nav-link ${isActive ? 'active' : ''}`}>
              <span className="material-symbols-outlined">category</span> Categories
            </NavLink>
            <NavLink to="/groups" className={({isActive}) => `nav-link ${isActive ? 'active' : ''}`}>
              <span className="material-symbols-outlined">group_work</span> Groups
            </NavLink>
            <NavLink to="/rules" className={({isActive}) => `nav-link ${isActive ? 'active' : ''}`}>
              <span className="material-symbols-outlined">rule</span> Title Rules
            </NavLink>
          </div>
        </nav>

        {/* Main Content */}
        <main className="main-content">
          <Routes>
            <Route path="/" element={<Dashboard />} />
            <Route path="/history" element={<HistoryPage />} />
            <Route path="/categories" element={<Categories />} />
            <Route path="/groups" element={<Groups />} />
            <Route path="/rules" element={<Rules />} />
            <Route path="*" element={<div className="page-header"><h1 className="page-title text-red-500">404 - Page Not Found</h1></div>} />
          </Routes>
        </main>
      </div>
    </BrowserRouter>
  );
}

export default App;
