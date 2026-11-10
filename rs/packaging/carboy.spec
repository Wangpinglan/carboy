Name:           carboy
Version:        0.1.1
Release:        1%{?dist}
Summary:        为 Wine / Proton 应用生成可用的 .desktop
License:        MIT
URL:            https://github.com/Wangpinglan/carboy
Source0:        %{name}-%{version}.tar.gz

BuildRequires:  rust >= 1.85
BuildRequires:  cargo
BuildRequires:  gtk4-devel >= 4.10
BuildRequires:  libadwaita-devel >= 1.5
Requires:       gtk4 >= 4.10
Requires:       libadwaita >= 1.5

%description
Carboy 是一个图形化的 .desktop 生成器，专门解决 Wine / Proton 应用的
桌面集成问题：自动推导窗口标识（StartupWMClass），从 exe 的 PE 资源里
抠出图标，装进 hicolor 多档尺寸并刷新缓存。

生成结果能被 GNOME / KDE / XFCE 正确识别，不再出现齿轮占位图标、
dock 里两个应用挤成一个、或者图标怎么也刷不出来。

%prep
rm -rf %{name}-%{version}
TARBALL=$(echo %{_sourcedir}/*.tar.gz)
/usr/bin/tar --no-same-owner --no-same-permissions -xzf "$TARBALL" || :
cd %{name}-%{version}

%build
cd %{name}-%{version}
%if 0%{?carboy_offline}
cargo build --release --offline
%else
cargo build --release
%endif

%install
cd %{name}-%{version}
install -Dm755 target/release/carboy %{buildroot}%{_bindir}/carboy
install -Dm644 LICENSE %{buildroot}%{_licensedir}/%{name}/LICENSE
install -Dm644 data/io.github.carboy.MkDesktop.desktop \
    %{buildroot}%{_datadir}/applications/io.github.carboy.MkDesktop.desktop
install -Dm644 data/io.github.carboy.MkDesktop.svg \
    %{buildroot}%{_datadir}/icons/hicolor/scalable/apps/io.github.carboy.MkDesktop.svg
install -Dm644 data/io.github.carboy.MkDesktop.metainfo.xml \
    %{buildroot}%{_datadir}/metainfo/io.github.carboy.MkDesktop.metainfo.xml

%files
%license LICENSE
%{_bindir}/carboy
%{_datadir}/applications/io.github.carboy.MkDesktop.desktop
%{_datadir}/icons/hicolor/scalable/apps/io.github.carboy.MkDesktop.svg
%{_datadir}/metainfo/io.github.carboy.MkDesktop.metainfo.xml

%changelog
* Tue Nov 10 2026 Carboy contributors <carboy@example.invalid> - 0.1.1-1
- 修复：管理页重复行、环境变量删错行、删行后预览不刷新
- 修复：umu 预装后路径丢失、GAMEID 重置按钮、新建按钮接通
- 新增：图标显示源档位与色深，窗口选择器显示 XID

* Tue Nov 10 2026 Carboy contributors <carboy@example.invalid> - 0.1.0-1
- 首个版本：桌面项生成、窗口标识推导、PE 图标提取、umu / wine 构建入口
