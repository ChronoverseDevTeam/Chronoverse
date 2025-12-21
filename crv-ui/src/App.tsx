import { createBrowserRouter, RouterProvider, Link, Outlet, useLocation } from 'react-router-dom'
import About from './pages/About'
import NotFound from './pages/NotFound'
import HiveManager from './pages/HiveManager'
import DepotTreeViewer from './pages/DepotTreeViewer'

const router = createBrowserRouter([
  {
    path: '/',
    element: <RootLayout />,
    children: [
      {
        element: <HiveManager />,
        children: [{ index: true, element: <DepotTreeViewer /> }],
      },
      { path: 'about', element: <About /> },
      { path: '*', element: <NotFound /> },
    ],
  },
])

const navItems = [
  { to: '/', label: 'Hive 控制台', matcher: (path: string) => path === '/' },
  { to: '/about', label: '关于', matcher: (path: string) => path.startsWith('/about') },
]

function RootLayout() {
  const location = useLocation()

  return (
    <div className="min-h-screen bg-slate-50 text-slate-900">
      <header className="border-b border-slate-200 bg-white/80 backdrop-blur">
        <div className="mx-auto flex max-w-7xl flex-wrap items-center justify-between gap-4 px-6 py-4">
          <div className="flex items-center gap-3">
            <div className="flex h-11 w-11 items-center justify-center rounded-2xl bg-slate-900 text-lg font-semibold text-white shadow-sm">
              CRV
            </div>
            <div>
              <div className="text-base font-semibold">Chronoverse</div>
              <p className="text-xs text-slate-500">Time To Refine VCS</p>
            </div>
          </div>

          <nav className="flex items-center gap-2">
            {navItems.map((item) => {
              const active = item.matcher(location.pathname)
              return (
                <Link
                  key={item.to}
                  to={item.to}
                  className={`rounded-lg px-4 py-2 text-sm font-medium transition ${
                    active
                      ? 'bg-slate-900 text-white shadow-sm'
                      : 'text-slate-600 hover:bg-slate-100 hover:text-slate-900'
                  }`}
                >
                  {item.label}
                </Link>
              )
            })}
          </nav>

          <div className="flex items-center gap-2 rounded-full border border-slate-200 px-3 py-1.5 text-xs font-medium text-slate-600">
            <span className="h-2 w-2 rounded-full bg-emerald-500" />
            Beta 预览
          </div>
        </div>
      </header>

      <main className="mx-auto max-w-7xl px-6 pb-12 pt-6">
        <Outlet />
      </main>
    </div>
  )
}

function App() {
  return <RouterProvider router={router} />
}

export default App
