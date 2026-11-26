// The Controller QML talks to, and the calls Rust makes into the window.
#include "gridview_p.h"

#include <QtGui/QRegion>
#include <QtGui/QWindow>

#include <QtCore/QPointer>
#include <QtCore/QProcess>
#include <QtCore/QTimer>
#include <QtGui/QFontMetricsF>
#include <QtGui/QGuiApplication>
#include <QtGui/QClipboard>
#include <QtGui/QInputMethodEvent>
#include <QtGui/QKeyEvent>
#include <QtGui/QGlyphRun>
#include <QtGui/QPainter>
#include <QtGui/QPainterPath>
#include <QtGui/QTextLayout>
#include <QtQml/qqml.h>
#include <QtQuick/QQuickWindow>
#include <QtQuick/QSGGeometryNode>
#include <QtQuick/QSGMaterial>
#include <QtQuick/QSGMaterialShader>
#include <QtQuick/QSGSimpleTextureNode>
#include <QtQuick/QSGTexture>
#include <QtQuick/QSGVertexColorMaterial>
#include <algorithm>
#include <cmath>
#include <cstring>

#include "kyko/src/bridge.cxx.h"

QList<QPointer<GridView>> g_views;
QPointer<Controller> g_ctl;

void grid_request_repaint() {
    for (const QPointer<GridView>& p : std::as_const(g_views))
        if (GridView* v = p)
            QMetaObject::invokeMethod(v, [v] { v->update(); v->refreshHeader(); }, Qt::QueuedConnection);
}

void controller_copy(rust::Str text, bool primary) {
    const QString s = QString::fromUtf8(text.data(), int(text.size()));
    QMetaObject::invokeMethod(
        qApp,
        [s, primary] {
            QClipboard* c = QGuiApplication::clipboard();
            if (!primary)
                c->setText(s, QClipboard::Clipboard);
            else if (c->supportsSelection())
                c->setText(s, QClipboard::Selection);
        },
        Qt::QueuedConnection);
}

void grid_bell(quint32 pane, int mode) {
    for (const QPointer<GridView>& p : std::as_const(g_views))
        if (GridView* v = p)
            QMetaObject::invokeMethod(
                v, [v, pane, mode] {
                    if (quint32(v->pane()) == pane)
                        v->bell(mode);
                },
                Qt::QueuedConnection);
}

void controller_paste(quint32 pane) {
    QMetaObject::invokeMethod(
        qApp,
        [pane] {
            const std::string s = QGuiApplication::clipboard()->text().toStdString();
            if (!s.empty())
                kyko_paste(pane, rust::Str(s.data(), s.size()));
        },
        Qt::QueuedConnection);
}

void controller_layout_changed() {
    if (Controller* c = g_ctl)
        QMetaObject::invokeMethod(c, [c] { Q_EMIT c->layoutChanged(); }, Qt::QueuedConnection);
}

void controller_send(int code) {
    if (Controller* c = g_ctl)
        QMetaObject::invokeMethod(c, [c, code] { Q_EMIT c->command(code); }, Qt::QueuedConnection);
}

void grid_reload_look() {
    for (const QPointer<GridView>& p : std::as_const(g_views))
        if (GridView* v = p)
            QMetaObject::invokeMethod(v, [v] { v->reloadLook(); }, Qt::QueuedConnection);
    if (Controller* c = g_ctl)
        QMetaObject::invokeMethod(c, [c] { Q_EMIT c->lookChanged(); }, Qt::QueuedConnection);
}

void grid_register() {
    qmlRegisterType<GridView>("org.kyko.app", 1, 0, "GridView");
    qmlRegisterType<Controller>("org.kyko.app", 1, 0, "Controller");
}

Controller::Controller(QObject* parent) : QObject(parent) {
    g_ctl = this;
    // The last shell can end before the window exists: the quit it asked for is still owed.
    if (kyko_exit_status() >= 0)
        QMetaObject::invokeMethod(this, [this] { Q_EMIT command(3); }, Qt::QueuedConnection);
}

Controller::~Controller() {
    if (g_ctl == this)
        g_ctl = nullptr;
}

int Controller::setting(const QString& name) const {
    const std::string n = name.toStdString();
    return kyko_setting(rust::Str(n.data(), n.size()));
}

void Controller::setVisible(bool visible) const { kyko_set_visible(visible); }

void Controller::setInputRegion(QObject* window, int x, int y, int w, int h) const {
    // On Wayland a window's mask is its surface's input region.
    if (auto* win = qobject_cast<QWindow*>(window))
        win->setMask(w > 0 && h > 0 ? QRegion(x, y, w, h) : QRegion());
}

QString Controller::layout() const { return str(kyko_layout()); }
void Controller::confirm(bool yes) const { kyko_confirm(yes); }
QString Controller::unicodeSearch(const QString& query) const {
    const QByteArray q = query.toUtf8();
    return str(kyko_unicode_search(rust::Str(q.data(), q.size())));
}
void Controller::unicodeDone(const QString& text, bool typed) const {
    const QByteArray t = text.toUtf8();
    kyko_unicode_done(rust::Str(t.data(), t.size()), typed);
}
QString Controller::paletteItems(const QString& query) const {
    const QByteArray q = query.toUtf8();
    return str(kyko_palette_items(rust::Str(q.data(), q.size())));
}
void Controller::paletteDone(const QString& id) const {
    const QByteArray t = id.toUtf8();
    kyko_palette_done(rust::Str(t.data(), t.size()));
}
void GridView::refreshHeader() {
    const QString h = str(kyko_sticky_header(quint32(m_pane)));
    if (h != m_header) {
        m_header = h;
        Q_EMIT stickyHeaderChanged();
    }
}
void Controller::search(int op, const QString& text) const {
    const std::string t = text.toStdString();
    kyko_search(quint8(op), rust::Str(t.data(), t.size()));
}
QString Controller::prefsPages() const { return str(kyko_prefs_pages()); }
QString Controller::prefsValues() const { return str(kyko_prefs_values()); }
QString Controller::prefsSet(const QString& key, const QString& json) const {
    const std::string k = key.toStdString(), j = json.toStdString();
    return str(kyko_prefs_set(rust::Str(k.data(), k.size()), rust::Str(j.data(), j.size())));
}
QString Controller::prefsReset(const QString& page) const {
    const std::string p = page.toStdString();
    return str(kyko_prefs_reset(rust::Str(p.data(), p.size())));
}
QString Controller::prefsKey(int key, int mods, const QString& text) const {
    const std::string t = text.toStdString();
    return str(kyko_prefs_key(quint32(key), quint32(mods), rust::Str(t.data(), t.size())));
}
QString Controller::shot() const { return str(kyko_test_shot()); }
int Controller::shotDelay() const {
    const int ms = qEnvironmentVariableIntValue("KYKO_TEST_MS");
    return ms > 0 ? ms : 1500;
}
QString Controller::fixedTitle() const { return str(kyko_fixed_title()); }
void Controller::action(const QString& name) const {
    const std::string n = name.toStdString();
    kyko_action(rust::Str(n.data(), n.size()));
}
void Controller::focusPane(int pane) const { kyko_focus_pane(quint32(pane)); }
void Controller::focusTab(int tab) const { kyko_focus_tab(quint32(tab)); }
void Controller::closeTab(int tab) const { kyko_close_tab(quint32(tab)); }
void Controller::closePane(int pane) const { kyko_close_pane(quint32(pane)); }
void Controller::dismiss(int pane) const { kyko_dismiss(quint32(pane)); }
void Controller::setRatio(int tab, const QString& path, double ratio) const {
    const std::string p = path.toStdString();
    kyko_set_ratio(quint32(tab), rust::Str(p.data(), p.size()), ratio);
}
void Controller::renameTab(int tab, const QString& name) const {
    const std::string n = name.toStdString();
    kyko_rename_tab(quint32(tab), rust::Str(n.data(), n.size()));
}
