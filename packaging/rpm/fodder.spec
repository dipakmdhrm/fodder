Name:           fodder
Version:        @VERSION@
Release:        1%{?dist}
Summary:        Lightweight RSS/Atom/JSON feed reader
License:        MIT
URL:            https://github.com/dipakmdhrm/fodder
Source0:        %{name}-%{version}.tar.gz

BuildRequires:  cargo
BuildRequires:  rust
BuildRequires:  gcc
BuildRequires:  pkgconfig(gtk4)
BuildRequires:  pkgconfig(libadwaita-1)
BuildRequires:  pkgconfig(webkitgtk-6.0)

Requires:       gtk4
Requires:       libadwaita
Requires:       webkitgtk6.0

%description
Fodder is a frugal feed reader for the Linux desktop: a headless daemon
(fodderd) polls feeds and sends notifications, and a GTK4 + libadwaita viewer
(fodder) is spawned on demand and freed on close, so idle memory stays low.

%prep
%autosetup

%build
cargo build --release --workspace --locked

%install
install -Dm 755 target/release/fodderd %{buildroot}%{_bindir}/fodderd
install -Dm 755 target/release/fodder  %{buildroot}%{_bindir}/fodder
install -Dm 644 data/applications/io.github.dipakmdhrm.Fodder.desktop \
    %{buildroot}%{_datadir}/applications/io.github.dipakmdhrm.Fodder.desktop

for size in 16 24 32 48 64 128 256 512; do
    install -Dm 644 data/icons/hicolor/${size}x${size}/apps/io.github.dipakmdhrm.Fodder.png \
        %{buildroot}%{_datadir}/icons/hicolor/${size}x${size}/apps/io.github.dipakmdhrm.Fodder.png
done
install -Dm 644 data/icons/hicolor/scalable/apps/io.github.dipakmdhrm.Fodder.svg \
    %{buildroot}%{_datadir}/icons/hicolor/scalable/apps/io.github.dipakmdhrm.Fodder.svg

%files
%license LICENSE
%{_bindir}/fodderd
%{_bindir}/fodder
%{_datadir}/applications/io.github.dipakmdhrm.Fodder.desktop
%{_datadir}/icons/hicolor/*/apps/io.github.dipakmdhrm.Fodder.*

%post
update-desktop-database %{_datadir}/applications 2>/dev/null || true
gtk-update-icon-cache -f -t %{_datadir}/icons/hicolor 2>/dev/null || true

%preun
# $1 == 0 on a full uninstall (not an upgrade).
if [ "$1" -eq 0 ]; then
    pkill -x fodderd 2>/dev/null || true
    pkill -x fodder 2>/dev/null || true
fi

%postun
if [ "$1" -eq 0 ]; then
    for home_dir in /home/*; do
        rm -f "$home_dir/.config/autostart/fodder.desktop" 2>/dev/null || true
    done
    update-desktop-database %{_datadir}/applications 2>/dev/null || true
    gtk-update-icon-cache -f -t %{_datadir}/icons/hicolor 2>/dev/null || true
fi

%changelog
* @CHANGELOG_DATE@ dipakmdhrm <dipakmdhrm@gmail.com> - @VERSION@-1
- Release @VERSION@
