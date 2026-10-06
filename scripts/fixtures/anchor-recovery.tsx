import { createRoot } from 'react-dom/client';
import { useEffect, useState } from 'react';
import { AnchorRecoveryPanel } from '../../src/components/AnchorRecoveryPanel';
import type { Account } from '../../src/hooks/useAccounts';
import '../../src/App.css';
import '../../src/components/Settings.css';
// @ts-expect-error Test-only JavaScript mock.
import { mock } from './anchor-recovery-ipc.mjs';

const accounts = ['anchor', 'worker'].map(id => ({
    id, name: id === 'anchor' ? '手机锚 A（模拟）' : '正常账号 B（模拟）',
    kind: 'chatgpt_oauth', is_session_anchor: id === 'anchor',
    auth_json: {}, is_banned: false, is_token_invalid: false, is_logged_out: false,
})) as Account[];
function Fixture() {
    const [displayAccounts, setDisplayAccounts] = useState(accounts);
    useEffect(() => {
        const migrated = (event: Event) => setDisplayAccounts(previous => previous.map(account => ({
            ...account, is_session_anchor: account.id === (event as CustomEvent<string>).detail,
        })));
        window.addEventListener('anchor-mock-migrated', migrated);
        return () => window.removeEventListener('anchor-mock-migrated', migrated);
    }, []);
    return <main style={{ padding: 24, maxWidth: 950 }}>
    <h2>手机锚恢复验收 · 模拟数据</h2>
    <button onClick={() => { mock.blockedTarget = true; }}>模拟目标不可用</button>
    <button onClick={() => { mock.blockedTarget = false; }}>模拟正常目标</button>
    <AnchorRecoveryPanel accounts={displayAccounts} />
    </main>;
}
createRoot(document.getElementById('root')!).render(<Fixture />);
