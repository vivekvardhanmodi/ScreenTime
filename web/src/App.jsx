import { BrowserRouter, Routes, Route, NavLink } from 'react-router-dom';
import { LayoutDashboard, History, Tags, Combine, Scissors } from 'lucide-react';
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
        <aside className="sidebar">
          <div className="sidebar-logo">
            <LayoutDashboard size={28} />
            ScreenTime
          </div>
          <nav className="nav-links">
            <NavLink to="/" className={({isActive}) => `nav-link ${isActive ? 'active' : ''}`}>
              <LayoutDashboard size={20} /> Dashboard
            </NavLink>
            <NavLink to="/history" className={({isActive}) => `nav-link ${isActive ? 'active' : ''}`}>
              <History size={20} /> History
            </NavLink>
            <NavLink to="/categories" className={({isActive}) => `nav-link ${isActive ? 'active' : ''}`}>
              <Tags size={20} /> Categories
            </NavLink>
            <NavLink to="/groups" className={({isActive}) => `nav-link ${isActive ? 'active' : ''}`}>
              <Combine size={20} /> Groups
            </NavLink>
            <NavLink to="/rules" className={({isActive}) => `nav-link ${isActive ? 'active' : ''}`}>
              <Scissors size={20} /> Title Rules
            </NavLink>
          </nav>
        </aside>

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
