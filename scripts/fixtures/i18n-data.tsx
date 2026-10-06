import { useState } from 'react';
import { createRoot } from 'react-dom/client';
import { installAppLocale } from '../../src/i18n';
import { LanguagePicker } from '../../src/components/LanguagePicker';
import '../../src/App.css';

installAppLocale('ru-RU');

function Fixture() {
    const [name, setName] = useState('生产账号A');
    return <main style={{ padding: 24 }}>
        <LanguagePicker />
        <h2>账号</h2>
        <p>复制 <span translate="no" title={name} data-testid="account-name">{name}</span></p>
        <p translate="no" data-testid="user-notes">客户备注：测试账号。保持原文。</p>
        <p data-i18n-ignore data-testid="ignored-data">自定义账号，测试。</p>
        <button onClick={() => setName('客户测试账号B')}>刷新</button>
    </main>;
}

createRoot(document.getElementById('root')!).render(<Fixture />);
