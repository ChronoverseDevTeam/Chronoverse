import type { ReactElement } from 'react'

type TreeNode = {
  path: string
  label: string
  type: 'folder' | 'file'
  children?: TreeNode[]
  badge?: string
}

type FileDetail = {
  name: string
  type: string
  change: string
  size: string
  summary: string
  updated: string
  owner: string
  status: '未锁定' | '锁定'
}

const tree: TreeNode[] = [
  {
    path: '//apps',
    label: 'apps',
    type: 'folder',
    badge: '5',
    children: [
      { path: '//apps/console', label: 'console', type: 'folder', badge: '2' },
      { path: '//apps/web', label: 'web', type: 'folder', badge: '3' },
    ],
  },
  {
    path: '//core',
    label: 'core',
    type: 'folder',
    badge: '8',
    children: [
      { path: '//core/runtime', label: 'runtime', type: 'folder', badge: '3' },
      { path: '//core/cache', label: 'cache', type: 'folder', badge: '2' },
      { path: '//core/edge', label: 'edge', type: 'folder', badge: '3' },
    ],
  },
  {
    path: '//docs',
    label: 'docs',
    type: 'folder',
    badge: '12',
  },
]

const files: FileDetail[] = [
  {
    name: 'timeline.proto',
    type: '协议',
    change: '+24/-3',
    size: '32 KB',
    summary: '新增 Snapshot RPC，精简字段映射',
    updated: '12月18日 14:22',
    owner: '程一',
    status: '未锁定',
  },
  {
    name: 'manifest.json',
    type: '配置',
    change: '+4/-0',
    size: '3.2 KB',
    summary: '声明 edge 节点与缓存策略',
    updated: '12月18日 10:01',
    owner: 'Ops',
    status: '未锁定',
  },
  {
    name: 'shard_allocator.go',
    type: '源码',
    change: '+182/-44',
    size: '24 KB',
    summary: '一致性哈希改为区间分片，减少抖动',
    updated: '12月17日 21:40',
    owner: '你',
    status: '未锁定',
  },
  {
    name: 'ui-theme.tokens.json',
    type: '设计',
    change: '+12/-6',
    size: '18 KB',
    summary: '扁平化 Token，统一半径和间距',
    updated: '12月17日 16:20',
    owner: 'Liya',
    status: '锁定',
  },
]

const statusStyle: Record<FileDetail['status'], string> = {
  未锁定: 'text-emerald-700 bg-emerald-50 border-emerald-200',
  锁定: 'text-rose-700 bg-rose-50 border-rose-200',
}

const activePath = '//core/runtime'
const activeRevision = '1031'

function renderTree(nodes: TreeNode[], level = 0): ReactElement[] {
  return nodes.flatMap((node) => {
    const active = node.path === activePath
    const current = (
      <div
        key={node.path}
        className={`group flex items-center justify-between rounded-lg px-2.5 py-1.5 text-xs hover:bg-slate-50 ${
          active ? 'border border-slate-200 bg-white shadow-sm' : ''
        }`}
        style={{ paddingLeft: `${level * 10 + 6}px`, marginLeft: level === 0 ? 0 : 2 }}
      >
        <div className="flex items-center gap-2">
          <span className="text-base">{node.type === 'folder' ? '📁' : '🗂️'}</span>
          <div className={`font-semibold ${active ? 'text-slate-900' : 'text-slate-800'}`}>{node.label}</div>
        </div>
        {node.badge && (
          <span
            className={`rounded-full px-2 py-0.5 text-[10px] font-medium ${
              active ? 'bg-slate-900 text-white' : 'bg-slate-100 text-slate-600'
            }`}
          >
            {node.badge}
          </span>
        )}
      </div>
    )

    const children: ReactElement[] = node.children ? renderTree(node.children, level + 1) : []
    return [current, ...children]
  })
}

function DepotTreeViewer() {
  return (
    <section className="rounded-3xl border border-slate-200 bg-white shadow-sm">
      <div className="flex flex-wrap items-center gap-2 border-b border-slate-200 px-4 py-2.5">
        <div className="relative flex-1 min-w-[280px]">
          <input
            placeholder="//a/b/c/...@12341"
            className="w-full rounded-lg border border-slate-200 bg-slate-50 px-3 py-2 text-sm font-mono text-slate-800 outline-none transition focus:border-slate-900 focus:bg-white"
          />
          <span className="pointer-events-none absolute right-2.5 top-1/2 -translate-y-1/2 text-[11px] text-slate-400">⌘ K</span>
        </div>

        <select className="min-w-[130px] rounded-lg border border-slate-200 bg-white px-3 py-2 text-xs font-medium text-slate-700 shadow-sm outline-none transition hover:border-slate-300 focus:border-slate-900">
          <option>main</option>
          <option>develop</option>
          <option>release/1.2</option>
        </select>

        <button className="rounded-lg border border-slate-900 bg-slate-900 px-3.5 py-2 text-xs font-semibold text-white shadow-sm transition hover:-translate-y-0.5">
          Go To
        </button>
      </div>

      <div className="grid gap-3 px-4 py-3 lg:grid-cols-[280px_minmax(0,1fr)]">
        <div className="rounded-xl border border-slate-200 bg-slate-50/60 p-2.5">
          <div className="mb-1.5 flex items-center justify-between text-[11px] font-semibold uppercase tracking-wide text-slate-500">
            <span>文件树</span>
            <span className="rounded-full bg-slate-100 px-2 py-0.5 text-[10px] font-medium text-slate-600">节点 3 / 18</span>
          </div>
          <div className="space-y-0.5 text-xs text-slate-800">{renderTree(tree)}</div>
        </div>

        <div className="rounded-xl border border-slate-200 bg-white p-2.5 shadow-inner">
          <div className="mb-2 flex flex-wrap items-center gap-1.5 text-[11px] text-slate-500">
            <span className="rounded-full bg-slate-100 px-2 py-0.5 font-medium text-slate-700">
              路径 {activePath}@{activeRevision}
            </span>
            <span className="rounded-full border border-emerald-200 bg-emerald-50 px-2 py-0.5 font-medium text-emerald-700">同步 80%</span>
            <span className="rounded-full border border-amber-200 bg-amber-50 px-2 py-0.5 font-medium text-amber-700">延迟 &lt; 240ms</span>
          </div>

          <div className="overflow-auto">
            <table className="w-full text-[12px] text-slate-800 lg:text-[13px]">
              <thead className="bg-slate-50 text-[11px] font-semibold uppercase tracking-wide text-slate-500">
                <tr>
                  <th className="px-2.5 py-1.5 text-left">名称</th>
                  <th className="px-2.5 py-1.5 text-left">类型</th>
                  <th className="px-2.5 py-1.5 text-left">变更</th>
                  <th className="px-2.5 py-1.5 text-left">大小</th>
                  <th className="px-2.5 py-1.5 text-left">描述</th>
                  <th className="px-2.5 py-1.5 text-left">更新时间</th>
                  <th className="px-2.5 py-1.5 text-left">所有者</th>
                  <th className="px-2.5 py-1.5 text-left">状态</th>
                </tr>
              </thead>
              <tbody className="divide-y divide-slate-100">
                {files.map((file) => (
                  <tr key={file.name} className="transition hover:bg-slate-50">
                    <td className="px-2.5 py-1.5 font-semibold text-slate-900">{file.name}</td>
                    <td className="px-2.5 py-1.5 text-slate-600">{file.type}</td>
                    <td className="px-2.5 py-1.5 text-slate-600">{file.change}</td>
                    <td className="px-2.5 py-1.5 text-slate-600">{file.size}</td>
                    <td className="px-2.5 py-1.5 text-slate-700">{file.summary}</td>
                    <td className="px-2.5 py-1.5 text-slate-600">{file.updated}</td>
                    <td className="px-2.5 py-1.5 text-slate-600">{file.owner}</td>
                    <td className="px-2.5 py-1.5">
                      <span className={`inline-flex items-center gap-1 rounded-full border px-2 py-0.5 text-[10px] font-semibold ${statusStyle[file.status]}`}>
                        <span className="h-1.5 w-1.5 rounded-full bg-current" />
                        {file.status}
                      </span>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </div>
      </div>
    </section>
  )
}

export default DepotTreeViewer

