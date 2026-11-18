<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset=".github/assets/banner-ko-dark.png">
    <img alt="Oink — Oink Is Not only a KVM. 당신의 컴퓨터를, 하나로." src=".github/assets/banner-ko-light.png" width="860">
  </picture>
</p>

# Oink — Oink Is Not only a KVM

[English](README.md) | [简体中文](README.zh-CN.md) | [日本語](README.ja.md) | **한국어**

Oink는 "Oink Is Not only a KVM"의 약자로, 여러 대의 Windows, Mac, Linux 사이에 P2P 공유 네트워크를 구축하는 최신 소프트웨어입니다. 어느 기기의 키보드, 마우스, 트랙패드로든 가까운 다른 기기를 조작할 수 있으며, 어디서나 통일된 네이티브 조작감을 유지합니다. 여러 기기의 오디오를 한 기기에 연결된 스피커나 헤드폰으로 재생할 수 있고, 클립보드 공유와 기기 간 드래그 앤 드롭, 로컬 네트워크 기기 자동 검색을 지원하며, 모든 통신은 기본적으로 암호화됩니다.

## 설치

[GitHub Releases](https://github.com/kshq1996/oink-linux-support/releases)에서 설치 파일과 해당 소스 코드를 받을 수 있습니다.

지원하는 운영체제와 버전:

- **Mac**: Apple 실리콘(arm64), macOS 14.2 이상. Intel(x64), macOS 14.2 이상(테스트되지 않음)
- **Windows**: x64, Windows 10 1809(빌드 17763) 이상 또는 Windows 11. 22H2 이전 Windows 10 버전은 테스트되지 않았습니다. Windows 10 LTSC(LTSC 2019 / LTSC 2021 등)에는 WebView2 Runtime이 기본으로 포함되지 않으므로, 먼저 Microsoft Edge WebView2 Runtime(Evergreen)을 설치하세요: https://developer.microsoft.com/microsoft-edge/webview2/
- **Linux**: x64 및 arm64, X11 또는 Wayland 데스크톱 세션 필요. 패키지는 Ubuntu 24.04에서 빌드되며 glibc 2.39 이상이 필요합니다. Debian 13 이상, Ubuntu 24.04 이상, Linux Mint 22 이상에서는 `.deb` 패키지를, Fedora 40 이상, openSUSE Tumbleweed에서는 `.rpm` 패키지를 설치합니다.

SmartScreen에 "Windows의 PC 보호" 메시지가 표시되면, 해당 옵션이 있을 때만 **추가 정보 → 실행**을 선택하세요. UAC에서 게시자를 알 수 없음으로 표시될 수 있습니다.

### Linux

Linux 패키지는 세 가지 프로그램을 설치합니다: 트레이 애플리케이션 `oink`, 명령줄 도구 `oink-agent`, 백그라운드 프로세스 `oink-backend`. 패키지는 런타임 의존성(WebKitGTK 4.1, GTK 3, libayatana-appindicator3)과 바이너리가 필요로 하는 C 라이브러리 버전을 선언하므로, 배포판 패키지 관리자가 빠진 부분을 설치하고 C 라이브러리가 오래된 시스템의 설치는 거부합니다.

패키지는 서명되어 있지 않습니다. `dnf install`과 `apt install`은 로컬의 서명되지 않은 패키지를 그대로 설치하지만, zypper는 무엇을 할지 묻다가 기본 답인 중단에서 멈추므로 다음과 같이 설치하세요:

```
sudo zypper --no-gpg-checks install ./Oink-<version>-linux-<arch>.rpm
```

Linux 백엔드는 클립보드에 X11 선택 프로토콜, 디스플레이 배치에 XRandR을 사용합니다. 입력 캡처와 주입은 세션에 따라 달라집니다: X11 세션에서는 XRecord와 XTest를 쓰고, Wayland 세션에서는 커널 입력 장치(`/dev/input`)를 읽어 캡처하고 가상 포인터 장치(`/dev/uinput`)에 써서 주입합니다(XWayland로는 컴포지터의 커서를 움직일 수 없기 때문입니다).

캡처는 마우스의 원시 카운트를 읽으므로 공유 포인터의 속도는 마우스 DPI와 컴포지터의 포인터 가속에 좌우됩니다. 데스크톱 설정에서 포인터 가속을 끄면 속도가 정확해집니다. macOS와 Windows가 그리는 커서 트레일 오버레이는 Linux에서 사용할 수 없습니다.

시스템 오디오는 Linux에서 아직 사용할 수 없습니다. 다른 기기의 오디오를 이 기기에서 재생하거나 마이크를 공유하려면 네이티브 오디오 백엔드가 필요한데, Linux에는 아직 없습니다. 키보드, 마우스, 클립보드, 디스플레이 배치, 기기 탐색 등 나머지 기능은 모두 동작합니다. 트랙패드는 X11 세션에서 캡처되며, Wayland 세션에서는 아직 제스처를 읽지 않습니다.

레이아웃 화면에서는 데스크톱 배경화일이 각 화면의 배경으로 그려집니다. 배경화일은 KDE Plasma 또는 GNOME 설정에서 읽습니다.

BazziteOS, Universal Blue, Fedora Silverblue 같은 불변의 Fedora 시스템에서는 루트 파일 시스템을 바꿀 수 없어 로컬 `.rpm`의 `dnf install`이 실패합니다. 대신 패키지를 레이어링하고 재부팅하세요:

```
sudo rpm-ostree install ./Oink-<version>-linux-<arch>.rpm
```

`rpm-ostree`는 로컬 패키지의 서명을 확인하지 않으므로 서명되지 않은 파일이 그대로 설치되고, 변경은 재부팅 후에 적용됩니다.

### 후원

이 프로젝트가 도움이 되셨다면 AI 구독료 일부를 후원해 주세요. 그 토큰 덕분에 Oink가 더 좋아질 수 있습니다.

<a href="https://ko-fi.com/hachiwii"><img alt="Ko-fi" src="https://ko-fi.com/img/githubbutton_sm.svg" height="36"></a>

## 기여

코드를 리뷰할 사람이 없기 때문에 맥락 설명이 없는 풀 리퀘스트는 받지 않습니다. 버그 보고와 기능 요청은 모두 [Issues](https://github.com/oinkvm/oink/issues)로 보내 주세요. 문제를 보고할 때는 기기 환경, 진단 데이터, 재현 절차, 그리고 AI로 조사한 결론을 함께 적어 주시면 좋습니다. 실제 환경에서 사용하고 피드백을 주시는 것이 이 프로젝트에 대한 가장 큰 기여입니다.

## 자주 묻는 질문

**Oink는 Deskflow 기반이거나 Deskflow와 호환되나요?**\
아니요. Oink는 처음부터 새로 만든 프로젝트로, 자체 개발한 P2P(클라이언트-서버 방식이 아닌) 프로토콜을 사용하며 다른 소프트웨어 KVM과 호환되지 않습니다.

**한 기기를 제어 기기로 지정해야 하나요?**\
아니요. Oink는 P2P 방식으로 설계되어 어느 기기에 연결된 키보드와 마우스로든 다른 기기를 제어할 수 있습니다.

**트랙패드 제스처를 공유할 수 있나요?**\
네. 다만 Windows 터치패드는 테스트되지 않았습니다.

**운영체제를 넘나드는 클립보드 공유와 기기 간 드래그 앤 드롭을 지원하나요?**\
네. 클립보드는 기기 간에 자동으로 동기화되며, 텍스트, 이미지, 파일을 한 기기에서 다른 기기로 바로 드래그할 수 있습니다. Chrome 같은 앱 안으로 바로 드롭할 수도 있습니다.

**개인 또는 상업적 용도로 사용하려면 라이선스를 구매해야 하나요?**\
아니요. Oink는 GPL-3.0-only(추가 조항 포함)로 배포되는 자유 소프트웨어로, 개인이든 상업적이든 어떤 용도로도 별도의 라이선스 없이 사용할 수 있습니다. Oink 또는 수정한 버전을 배포하는 경우에는 같은 라이선스로 전체 소스 코드를 제공하고, 원작자 표시와 라이선스 고지를 유지하며, 변경 사항을 명시해야 합니다. 추가 조항에 따라 파생 제품을 홍보하는 데 Oink라는 이름을 사용할 수도 없습니다. 배포하지 않고 직접 사용하기 위해서만 수정하는 경우에는 이러한 의무가 없습니다.

## 라이선스

이 프로젝트는 [GPL-3.0-only](LICENSE) 라이선스와 [추가 조항](LICENSE-ADDITIONAL-TERMS.md)에 따라 배포됩니다.
