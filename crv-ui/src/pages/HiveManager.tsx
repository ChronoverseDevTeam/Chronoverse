import { Outlet } from 'react-router-dom'

function HiveManager() {
  return (
    <div className="space-y-6">
      <section className="grid gap-6 rounded-3xl border border-slate-200 bg-white px-6 py-5 shadow-sm lg:grid-cols-[minmax(0,2.6fr)_minmax(300px,1fr)] items-stretch">
        <div className="space-y-4 min-w-[320px]">
          <div className="inline-flex items-center gap-2 rounded-full border border-slate-200 px-3 py-1 text-xs font-medium text-slate-600">
            <span className="h-2 w-2 rounded-full bg-emerald-500" />
            Chronoverse Hive / 工作区
          </div>
          <div className="space-y-1">
            <h1 className="text-3xl font-semibold tracking-tight text-slate-900">Hive Manager</h1>
            <p className="text-sm text-slate-500">
              Hive 状态控制中心
            </p>
          </div>
          <div className="grid gap-3 sm:grid-cols-2 lg:grid-cols-4">
            {[
              { label: '资源数量', value: '42 项', hint: '含 12 个热点' },
              { label: '存储用量', value: '8.2 GB', hint: '本周 +0.6 GB' },
              { label: '最近改动', value: '7 次', hint: '24 小时内' },
              { label: '用户数量', value: '20 个', hint: '在线用户 10 个' },
            ].map((card) => (
              <div
                key={card.label}
                className="rounded-2xl border border-slate-200 bg-slate-50 px-4 py-3 shadow-inner"
              >
                <div className="text-xs font-semibold uppercase tracking-wide text-slate-500">{card.label}</div>
                <div className="mt-1 text-2xl font-semibold text-slate-900">{card.value}</div>
                <div className="text-xs text-slate-500">{card.hint}</div>
              </div>
            ))}
          </div>
        </div>

        <div className="flex h-full flex-col gap-3 rounded-2xl border border-slate-200 bg-slate-900 text-white shadow-sm">
          <div className="flex items-center justify-between px-4 pt-4 text-xs font-medium uppercase tracking-wide text-slate-200">
            <span>Sync 频率</span>
            <span className="rounded-full bg-emerald-500/20 px-2 py-1 text-emerald-100">平稳</span>
          </div>
          <div className="space-y-1 px-4">
            <div className="flex items-baseline gap-2">
              <div className="text-3xl font-semibold">1031</div>
              <div className="text-sm text-slate-200">次同步 / 24h</div>
            </div>
            <p className="text-xs text-slate-200/80">最近的同步延迟小于 240ms，链路健康。</p>
          </div>
          <div className="px-4 pb-4">
            <div className="h-2 rounded-full bg-slate-800">
              <div className="h-full w-4/5 rounded-full bg-emerald-400"></div>
            </div>
            <div className="mt-2 flex items-center justify-between text-xs text-slate-200/80">
              <span>平均带宽占用 80%</span>
              <span>剩余 20%</span>
            </div>
          </div>
        </div>
      </section>
      <Outlet />
    </div>
  )
}

export default HiveManager

