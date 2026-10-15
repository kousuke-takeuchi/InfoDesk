/// The A-15 "NAS の準備" guide, as data. The texts are the same as
/// `app/src/views/settings/NasGuideDialog.tsx`.
class NasGuideStep {
  const NasGuideStep(this.title, this.body);
  final String title;
  final String body;
}

const nasGuideTitle = 'NAS の準備';
const nasGuideIntro = 'Synology NAS を InfoDesk の保存先にします。最初の1回だけ行います。';

const nasGuideSteps = <NasGuideStep>[
  NasGuideStep(
    'DSM で WebDAV Server を入れる',
    'パッケージセンターで「WebDAV Server」を入れ、HTTP (ポート 5005) を有効にします。',
  ),
  NasGuideStep('共有フォルダを作る', 'コントロールパネル → 共有フォルダで「infodesk」を作ります。'),
  NasGuideStep(
    'InfoDesk 用のユーザーを作る',
    'コントロールパネル → ユーザーで新しいユーザーを作り、infodesk だけ「読み取り/書き込み」、ほかの共有フォルダは「アクセス不可」にします。',
  ),
  NasGuideStep(
    'NAS に Tailscale を入れる',
    'パッケージセンターで「Tailscale」を入れてログインし、NAS の名前 (例: takeuchi-nas) を控えます。',
  ),
  NasGuideStep(
    'InfoDesk でつなぐ',
    '保存先に「NAS (WebDAV)」を選び、Tailscale にログイン (NAS と同じアカウント)。URL に http://NAS の名前:5005/infodesk/、ユーザー名とパスワードを入れて「確かめる」を押します。',
  ),
];

const nasGuideTip =
    'Mac や iPhone に Tailscale のアプリは要りません。InfoDesk の中で Tailscale を使うので、ルーターのポートを開けず、ほかのアプリの通信にも影響しません。';

const nasGuideLinkLabel = '詳しい手順 (docs/13-nas-setup)';
const nasGuideUrl =
    'https://github.com/kousuke-takeuchi/InfoDesk/blob/master/docs/13-nas-setup.ja.md';
