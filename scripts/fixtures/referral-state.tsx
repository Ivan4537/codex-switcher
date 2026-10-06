import { createRoot } from 'react-dom/client';
import { useState } from 'react';
import { ReferralQuotaCard } from '../../src/components/ReferralQuotaCard';
import { ReferralInviteModal } from '../../src/components/ReferralInviteModal';
import '../../src/App.css';
// @ts-expect-error Test-only JS module.
import { mock } from './referral-state-ipc.mjs';

function Fixture() {
    const [id, setId] = useState('same-account');
    const [showModal, setShowModal] = useState(false);
    return <main style={{ padding: 24, maxWidth: 900 }}>
        <h2>邀请状态验收 · 模拟数据</h2>
        <button onClick={() => { mock.mode = 'blocked'; }}>模拟 403</button>
        <button onClick={() => { mock.mode = 'available'; }}>模拟 3 次无奖励邀请</button>
        <button onClick={() => { mock.mode = 'incomplete'; }}>模拟字段缺失</button>
        <button onClick={() => { mock.mode = 'inactive'; }}>模拟明确未开放</button>
        <button onClick={() => setId('other-account')}>切换模拟账号</button>
        <button onClick={() => setShowModal(true)}>打开模拟邀请窗口</button>
        <ReferralQuotaCard accountId={id} program="codex_referral_consumer" />
        {showModal && <ReferralInviteModal id={id} name="模拟账号（非真实账号）" program="codex_referral_consumer" onClose={() => setShowModal(false)} />}
    </main>;
}
createRoot(document.getElementById('root')!).render(<Fixture />);
