import { StrictMode, useState } from 'react';
import { createRoot } from 'react-dom/client';
import { UsageCard } from '../../src/components/UsageCard';
import type { UsageDisplay } from '../../src/hooks/useUsage';
import '../../src/App.css';

const freeUsage: UsageDisplay = {
    plan_type: 'Free', five_hour_used: 27, five_hour_left: 73,
    five_hour_reset: '未知', five_hour_reset_at: Math.floor(Date.now() / 1000) + 600,
    weekly_used: 0, weekly_left: 100, weekly_reset: '未知',
    credits_balance: null, has_credits: false,
};

function Fixture() {
    const [scene, setScene] = useState('loading');
    return <main style={{ padding: 24, maxWidth: 600 }}>
        <h3>额度状态切换 · 示例数据</h3>
        <p>当前状态：{scene}</p>
        <button onClick={() => setScene('loading')}>加载</button>
        <button onClick={() => setScene('success')}>Free 成功</button>
        <button onClick={() => setScene('error')}>报错</button>
        <button onClick={() => setScene('empty')}>无数据</button>
        <UsageCard usage={scene === 'success' || scene === 'error' ? freeUsage : null}
            loading={scene === 'loading'} error={scene === 'error' ? '模拟刷新失败' : null}
            onRefresh={() => setScene('success')} />
    </main>;
}

createRoot(document.getElementById('root')!).render(<StrictMode><Fixture /></StrictMode>);
