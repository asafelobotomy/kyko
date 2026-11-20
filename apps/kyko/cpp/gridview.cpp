#include "gridview.h"

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

namespace {
const quint8 BOLD = 1, ITALIC = 2, UNDERLINE = 4, WIDE = 16, WIDE_SPACER = 32;
const int ATLAS = 2048;
QList<QPointer<GridView>> g_views;
QPointer<Controller> g_ctl;

// ---- glyph material: alpha from the atlas, colour per vertex ------------------------------------
struct GlyphVertex {
    float x, y, u, v;
    uchar r, g, b, a;
};

class GlyphMaterial : public QSGMaterial {
public:
    GlyphMaterial() { setFlag(Blending, true); }
    QSGMaterialType* type() const override {
        static QSGMaterialType t;
        return &t;
    }
    QSGMaterialShader* createShader(QSGRendererInterface::RenderMode) const override;
    int compare(const QSGMaterial* o) const override {
        return texture == static_cast<const GlyphMaterial*>(o)->texture ? 0 : 1;
    }
    QSGTexture* texture = nullptr;
};

class GlyphShader : public QSGMaterialShader {
public:
    GlyphShader() {
        setShaderFileName(VertexStage, QStringLiteral(":/kyko/shaders/glyph.vert.qsb"));
        setShaderFileName(FragmentStage, QStringLiteral(":/kyko/shaders/glyph.frag.qsb"));
    }
    bool updateUniformData(RenderState& state, QSGMaterial*, QSGMaterial*) override {
        QByteArray* buf = state.uniformData();
        bool changed = false;
        if (state.isMatrixDirty()) {
            const QMatrix4x4 m = state.combinedMatrix();
            std::memcpy(buf->data(), m.constData(), 64);
            changed = true;
        }
        if (state.isOpacityDirty()) {
            const float o = state.opacity();
            std::memcpy(buf->data() + 64, &o, 4);
            changed = true;
        }
        return changed;
    }
    void updateSampledImage(RenderState& state, int binding, QSGTexture** texture, QSGMaterial* m,
                            QSGMaterial*) override {
        if (binding != 1)
            return;
        QSGTexture* t = static_cast<GlyphMaterial*>(m)->texture;
        t->commitTextureOperations(state.rhi(), state.resourceUpdateBatch());
        *texture = t;
    }
};

QSGMaterialShader* GlyphMaterial::createShader(QSGRendererInterface::RenderMode) const { return new GlyphShader; }

const QSGGeometry::AttributeSet& glyphAttributes() {
    static QSGGeometry::Attribute a[] = {
        QSGGeometry::Attribute::createWithAttributeType(0, 2, QSGGeometry::FloatType, QSGGeometry::PositionAttribute),
        QSGGeometry::Attribute::createWithAttributeType(1, 2, QSGGeometry::FloatType, QSGGeometry::TexCoordAttribute),
        QSGGeometry::Attribute::createWithAttributeType(2, 4, QSGGeometry::UnsignedByteType, QSGGeometry::ColorAttribute),
    };
    static QSGGeometry::AttributeSet set = {3, sizeof(GlyphVertex), a};
    return set;
}

inline void quad(QSGGeometry::ColoredPoint2D*& p, float x0, float y0, float x1, float y1, quint32 c) {
    const uchar r = (c >> 16) & 0xff, g = (c >> 8) & 0xff, b = c & 0xff;
    p[0].set(x0, y0, r, g, b, 255);
    p[1].set(x1, y0, r, g, b, 255);
    p[2].set(x0, y1, r, g, b, 255);
    p[3].set(x1, y0, r, g, b, 255);
    p[4].set(x1, y1, r, g, b, 255);
    p[5].set(x0, y1, r, g, b, 255);
    p += 6;
}

QSGGeometryNode* makeBgNode() {
    auto* n = new QSGGeometryNode;
    auto* g = new QSGGeometry(QSGGeometry::defaultAttributes_ColoredPoint2D(), 0);
    g->setDrawingMode(QSGGeometry::DrawTriangles);
    g->setVertexDataPattern(QSGGeometry::StreamPattern);
    n->setGeometry(g);
    n->setMaterial(new QSGVertexColorMaterial);
    n->setFlags(QSGNode::OwnsGeometry | QSGNode::OwnsMaterial);
    return n;
}

// Backgrounds: one quad for the whole item, one per run of cells with a non-default background,
// and the cursor block on top.
// The gutter: a thin bar at the left edge of the rows that belong to a command the shell marked,
// green when it succeeded, red when it failed, yellow while it runs.
constexpr float GUTTER_W = 3.0f;

void fillBg(QSGGeometryNode* node, const FfiCell* cells, const uint8_t* gutter, int cols, int rows, qreal cw,
            qreal ch, qreal w, qreal h, int curX, int curY, quint32 curColor, uchar alpha) {
    const quint32 def = kyko_palette_bg();
    int quads = 1 + (curX >= 0 ? 1 : 0);
    for (int r = 0; r < rows; ++r)
        if (gutter[r])
            ++quads;
    for (int r = 0; r < rows; ++r) {
        const FfiCell* row = cells + size_t(r) * cols;
        for (int c = 0; c < cols;) {
            int e = c + 1;
            while (e < cols && row[e].bg == row[c].bg)
                ++e;
            if (row[c].bg != def)
                ++quads;
            c = e;
        }
    }
    QSGGeometry* g = node->geometry();
    g->allocate(quads * 6);
    auto* p = g->vertexDataAsColoredPoint2D();
    // The default background, see-through by `window.opacity_pct` (premultiplied, as the vertex
    // colour material expects); the cells a program coloured stay opaque.
    {
        const uchar r = uchar(((def >> 16) & 0xff) * alpha / 255), g = uchar(((def >> 8) & 0xff) * alpha / 255),
                    b = uchar((def & 0xff) * alpha / 255);
        const float fw = float(w), fh = float(h);
        p[0].set(0, 0, r, g, b, alpha);
        p[1].set(fw, 0, r, g, b, alpha);
        p[2].set(0, fh, r, g, b, alpha);
        p[3].set(fw, 0, r, g, b, alpha);
        p[4].set(fw, fh, r, g, b, alpha);
        p[5].set(0, fh, r, g, b, alpha);
        p += 6;
    }
    for (int r = 0; r < rows; ++r) {
        const FfiCell* row = cells + size_t(r) * cols;
        for (int c = 0; c < cols;) {
            int e = c + 1;
            while (e < cols && row[e].bg == row[c].bg)
                ++e;
            if (row[c].bg != def)
                quad(p, float(c * cw), float(r * ch), float(e * cw), float((r + 1) * ch), row[c].bg);
            c = e;
        }
    }
    for (int r = 0; r < rows; ++r)
        if (gutter[r])
            quad(p, 0, float(r * ch), GUTTER_W, float((r + 1) * ch), kyko_palette_gutter(gutter[r]));
    if (curX >= 0) {
        const bool wide = cells[size_t(curY) * cols + curX].flags & WIDE;
        quad(p, float(curX * cw), float(curY * ch), float((curX + (wide ? 2 : 1)) * cw), float((curY + 1) * ch),
             curColor);
    }
    node->markDirty(QSGNode::DirtyGeometry);
}
}  // namespace

void grid_request_repaint() {
    for (const QPointer<GridView>& p : std::as_const(g_views))
        if (GridView* v = p)
            QMetaObject::invokeMethod(v, [v] { v->update(); }, Qt::QueuedConnection);
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

namespace {
QString str(const rust::String& s) { return QString::fromUtf8(s.data(), int(s.size())); }
}  // namespace

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

// ---- drawn shapes: box drawing, blocks and Powerline separators ----------------------------------
namespace {
// Box drawing U+2500..U+257F as four arms (up, right, down, left): 0 none, 1 light, 2 heavy.
// 0 marks a character drawn some other way (dashes, double lines, arcs, diagonals).
const char* const BOX_ARMS[128] = {
    "0101", "0202", "1010", "2020", nullptr, nullptr, nullptr, nullptr,  // 2500
    nullptr, nullptr, nullptr, nullptr, "0110", "0210", "0120", "0220",  // 2508
    "0011", "0012", "0021", "0022", "1100", "1200", "2100", "2200",      // 2510
    "1001", "1002", "2001", "2002", "1110", "1210", "2110", "1120",      // 2518
    "2120", "2210", "1220", "2220", "1011", "1012", "2011", "1021",      // 2520
    "2021", "2012", "1022", "2022", "0111", "0112", "0211", "0212",      // 2528
    "0121", "0122", "0221", "0222", "1101", "1102", "1201", "1202",      // 2530
    "2101", "2102", "2201", "2202", "1111", "1112", "1211", "1212",      // 2538
    "2111", "1121", "2121", "2112", "2211", "1122", "1221", "2212",      // 2540
    "1222", "2122", "2221", "2222", nullptr, nullptr, nullptr, nullptr,  // 2548
    nullptr, nullptr, nullptr, nullptr, nullptr, nullptr, nullptr, nullptr,  // 2550
    nullptr, nullptr, nullptr, nullptr, nullptr, nullptr, nullptr, nullptr,  // 2558
    nullptr, nullptr, nullptr, nullptr, nullptr, nullptr, nullptr, nullptr,  // 2560
    nullptr, nullptr, nullptr, nullptr, nullptr, nullptr, nullptr, nullptr,  // 2568
    nullptr, nullptr, nullptr, nullptr, "0001", "1000", "0100", "0010",  // 2570
    "0002", "2000", "0200", "0020", "0201", "2010", "0102", "1020",      // 2578
};

// Double lines U+2550..U+256C as arms: 0 none, 1 single, 3 double.
const char* const DOUBLE_ARMS[29] = {
    "0303", "3030", "0310", "0130", "0330", "0013", "0031", "0033",  // 2550
    "1300", "3100", "3300", "1003", "3001", "3003", "1310", "3130",  // 2558
    "3330", "1013", "3031", "3033", "0313", "0131", "0333", "1303",  // 2560
    "3101", "3303", "1313", "3131", "3333",                          // 2568
};

bool drawnShape(quint32 ch) {
    return (ch >= 0x2500 && ch <= 0x259f) || (ch >= 0xe0b0 && ch <= 0xe0bf);
}

// Draws `ch` into the w x h rectangle at (x, y); false when it is not one of the drawn shapes.
bool drawShape(QPainter& p, quint32 ch, int x, int y, int w, int h, int light) {
    const int heavy = std::max(light + 1, light * 2);
    const int cx = x + w / 2, cy = y + h / 2;
    const QColor white(Qt::white);
    auto thick = [&](char a) { return a == '2' ? heavy : a == '1' ? light : 0; };
    auto armsDraw = [&](const char* a) {
        const int up = thick(a[0]), right = thick(a[1]), down = thick(a[2]), left = thick(a[3]);
        const int vt = std::max(up, down), ht = std::max(left, right);
        if (up) p.fillRect(cx - up / 2, y, up, cy - y + (ht + 1) / 2 + (ht ? 0 : up / 2 + 1), white);
        if (down) p.fillRect(cx - down / 2, cy - (ht ? ht / 2 : down / 2), down, y + h - cy + (ht ? ht / 2 : down / 2), white);
        if (left) p.fillRect(x, cy - left / 2, cx - x + (vt + 1) / 2 + (vt ? 0 : left / 2 + 1), left, white);
        if (right) p.fillRect(cx - (vt ? vt / 2 : right / 2), cy - right / 2, x + w - cx + (vt ? vt / 2 : right / 2), right, white);
    };
    if (ch >= 0x2500 && ch <= 0x257f) {
        const int i = int(ch - 0x2500);
        if (BOX_ARMS[i]) {
            armsDraw(BOX_ARMS[i]);
            return true;
        }
        // Dashed lines: 2504-250b (triple, quadruple), 254c-254f (double).
        auto dashes = [&](bool vertical, int n, int t) {
            const int len = vertical ? h : w;
            const qreal seg = qreal(len) / n;
            for (int k = 0; k < n; ++k) {
                const int a = int(std::round(k * seg)), b = int(std::round(k * seg + seg * 0.6));
                if (vertical)
                    p.fillRect(cx - t / 2, y + a, t, std::max(1, b - a), white);
                else
                    p.fillRect(x + a, cy - t / 2, std::max(1, b - a), t, white);
            }
        };
        switch (ch) {
        case 0x2504: dashes(false, 3, light); return true;
        case 0x2505: dashes(false, 3, heavy); return true;
        case 0x2506: dashes(true, 3, light); return true;
        case 0x2507: dashes(true, 3, heavy); return true;
        case 0x2508: dashes(false, 4, light); return true;
        case 0x2509: dashes(false, 4, heavy); return true;
        case 0x250a: dashes(true, 4, light); return true;
        case 0x250b: dashes(true, 4, heavy); return true;
        case 0x254c: dashes(false, 2, light); return true;
        case 0x254d: dashes(false, 2, heavy); return true;
        case 0x254e: dashes(true, 2, light); return true;
        case 0x254f: dashes(true, 2, heavy); return true;
        default: break;
        }
        if (ch >= 0x2550 && ch <= 0x256c) {
            // Double lines: two strokes `gap` apart; at a junction each stroke stops at the
            // stroke it meets, so corners and tees close the way the font's would.
            const char* a = DOUBLE_ARMS[ch - 0x2550];
            const int g = std::max(light + 1, light * 2);  // centre to stroke
            const int t = light;
            auto hline = [&](int x0, int x1, int yy) { if (x1 > x0) p.fillRect(x0, yy - t / 2, x1 - x0, t, white); };
            auto vline = [&](int y0, int y1, int xx) { if (y1 > y0) p.fillRect(xx - t / 2, y0, t, y1 - y0, white); };
            const char up = a[0], right = a[1], down = a[2], left = a[3];
            const bool vDouble = up == '3' || down == '3', hDouble = left == '3' || right == '3';
            const int vr = vDouble ? g : 0;  // half-width of the vertical strokes
            const int hr = hDouble ? g : 0;
            // Horizontal arms.
            for (int side : {-1, 1}) {
                const char arm = side < 0 ? left : right;
                if (arm == '0')
                    continue;
                const int edge = side < 0 ? x : x + w;
                if (arm == '1') {
                    const int stop = cx + side * -vr;  // single meets the near vertical stroke
                    side < 0 ? hline(edge, stop + (vr ? 0 : (t + 1) / 2), cy) : hline(stop - (vr ? 0 : t / 2), edge, cy);
                    continue;
                }
                for (int lane : {-1, 1}) {
                    const int yy = cy + lane * g;
                    // The stroke on the side where the vertical arm comes from stops at the near
                    // vertical stroke; the other runs to the far one (or the centre).
                    const bool vertHere = (lane < 0 ? up : down) != '0';
                    const bool vertOther = (lane < 0 ? down : up) != '0';
                    int reach;
                    if (vertHere)
                        reach = -vr;  // stop at the near vertical stroke
                    else if (vertOther || vDouble)
                        reach = vr;   // run to the far stroke
                    else
                        reach = up == '1' || down == '1' ? 0 : 0;
                    if (!vertHere && !vertOther && !vDouble)
                        reach = (up == '0' && down == '0') ? (side < 0 ? (w - w / 2) : w / 2) : 0;
                    const int stop = cx - side * reach;
                    side < 0 ? hline(edge, stop + (t + 1) / 2, yy) : hline(stop - t / 2, edge, yy);
                }
            }
            for (int side : {-1, 1}) {
                const char arm = side < 0 ? up : down;
                if (arm == '0')
                    continue;
                const int edge = side < 0 ? y : y + h;
                if (arm == '1') {
                    const int stop = cy + side * -hr;
                    side < 0 ? vline(edge, stop + (hr ? 0 : (t + 1) / 2), cx) : vline(stop - (hr ? 0 : t / 2), edge, cx);
                    continue;
                }
                for (int lane : {-1, 1}) {
                    const int xx = cx + lane * g;
                    const bool horHere = (lane < 0 ? left : right) != '0';
                    const bool horOther = (lane < 0 ? right : left) != '0';
                    int reach;
                    if (horHere)
                        reach = -hr;
                    else if (horOther || hDouble)
                        reach = hr;
                    else
                        reach = (left == '0' && right == '0') ? (side < 0 ? (h - h / 2) : h / 2) : 0;
                    const int stop = cy - side * reach;
                    side < 0 ? vline(edge, stop + (t + 1) / 2, xx) : vline(stop - t / 2, edge, xx);
                }
            }
            return true;
        }
        p.save();
        p.setRenderHint(QPainter::Antialiasing, true);
        QPen pen(white, light, Qt::SolidLine, Qt::FlatCap, Qt::MiterJoin);
        p.setPen(pen);
        p.setBrush(Qt::NoBrush);
        const qreal fx = cx + (light % 2 ? 0.5 : 0.0), fy = cy + (light % 2 ? 0.5 : 0.0);
        if (ch >= 0x256d && ch <= 0x2570) {
            // Rounded corners: from the middle of one edge to the middle of the other.
            const qreal r = std::min(qreal(w) / 2, qreal(h) / 2);
            QPainterPath path;
            switch (ch) {
            case 0x256d:  // down and right
                path.moveTo(fx, y + h); path.lineTo(fx, fy + r); path.quadTo(fx, fy, fx + r, fy); path.lineTo(x + w, fy); break;
            case 0x256e:  // down and left
                path.moveTo(fx, y + h); path.lineTo(fx, fy + r); path.quadTo(fx, fy, fx - r, fy); path.lineTo(x, fy); break;
            case 0x256f:  // up and left
                path.moveTo(fx, y); path.lineTo(fx, fy - r); path.quadTo(fx, fy, fx - r, fy); path.lineTo(x, fy); break;
            default:  // up and right
                path.moveTo(fx, y); path.lineTo(fx, fy - r); path.quadTo(fx, fy, fx + r, fy); path.lineTo(x + w, fy); break;
            }
            p.drawPath(path);
        } else {
            if (ch == 0x2571 || ch == 0x2573) p.drawLine(QPointF(x + w, y), QPointF(x, y + h));
            if (ch == 0x2572 || ch == 0x2573) p.drawLine(QPointF(x, y), QPointF(x + w, y + h));
        }
        p.restore();
        return true;
    }
    if (ch >= 0x2580 && ch <= 0x259f) {
        auto rect = [&](qreal fx0, qreal fy0, qreal fx1, qreal fy1, int alpha = 255) {
            const int x0 = x + int(std::round(fx0 * w)), x1 = x + int(std::round(fx1 * w));
            const int y0 = y + int(std::round(fy0 * h)), y1 = y + int(std::round(fy1 * h));
            QColor c(255, 255, 255, alpha);
            p.fillRect(x0, y0, std::max(0, x1 - x0), std::max(0, y1 - y0), c);
        };
        const quint32 i = ch - 0x2580;
        if (i == 0) { rect(0, 0, 1, 0.5); return true; }
        if (i >= 1 && i <= 8) { rect(0, 1 - i / 8.0, 1, 1); return true; }
        if (i >= 9 && i <= 15) { rect(0, 0, (16 - i) / 8.0, 1); return true; }
        switch (ch) {
        case 0x2590: rect(0.5, 0, 1, 1); return true;
        case 0x2591: rect(0, 0, 1, 1, 64); return true;
        case 0x2592: rect(0, 0, 1, 1, 128); return true;
        case 0x2593: rect(0, 0, 1, 1, 192); return true;
        case 0x2594: rect(0, 0, 1, 1 / 8.0); return true;
        case 0x2595: rect(7 / 8.0, 0, 1, 1); return true;
        default: break;
        }
        // Quadrants 2596..259f: upper left, upper right, lower left, lower right bits.
        static const quint8 Q[10] = {4, 8, 1, 13, 9, 7, 11, 2, 6, 14};
        const quint8 q = Q[ch - 0x2596];
        if (q & 1) rect(0, 0, 0.5, 0.5);
        if (q & 2) rect(0.5, 0, 1, 0.5);
        if (q & 4) rect(0, 0.5, 0.5, 1);
        if (q & 8) rect(0.5, 0.5, 1, 1);
        return true;
    }
    if (ch >= 0xe0b0 && ch <= 0xe0bf) {
        p.save();
        p.setRenderHint(QPainter::Antialiasing, true);
        const qreal L = x, R = x + w, T = y, B = y + h, M = y + h / 2.0;
        QPainterPath path;
        bool fill = true;
        switch (ch) {
        case 0xe0b0: path.moveTo(L, T); path.lineTo(R, M); path.lineTo(L, B); path.closeSubpath(); break;
        case 0xe0b1: path.moveTo(L, T); path.lineTo(R, M); path.lineTo(L, B); fill = false; break;
        case 0xe0b2: path.moveTo(R, T); path.lineTo(L, M); path.lineTo(R, B); path.closeSubpath(); break;
        case 0xe0b3: path.moveTo(R, T); path.lineTo(L, M); path.lineTo(R, B); fill = false; break;
        case 0xe0b4: path.moveTo(L, T); path.arcTo(QRectF(L - w, T, 2 * w, h), 90, -180); path.closeSubpath(); break;
        case 0xe0b5: path.moveTo(L, T); path.arcTo(QRectF(L - w, T, 2 * w, h), 90, -180); fill = false; break;
        case 0xe0b6: path.moveTo(R, T); path.arcTo(QRectF(L, T, 2 * w, h), 90, 180); path.closeSubpath(); break;
        case 0xe0b7: path.moveTo(R, T); path.arcTo(QRectF(L, T, 2 * w, h), 90, 180); fill = false; break;
        case 0xe0b8: path.moveTo(L, T); path.lineTo(R, B); path.lineTo(L, B); path.closeSubpath(); break;
        case 0xe0b9: path.moveTo(L, T); path.lineTo(R, B); fill = false; break;
        case 0xe0ba: path.moveTo(R, T); path.lineTo(R, B); path.lineTo(L, B); path.closeSubpath(); break;
        case 0xe0bb: path.moveTo(R, T); path.lineTo(L, B); fill = false; break;
        case 0xe0bc: path.moveTo(L, T); path.lineTo(R, T); path.lineTo(L, B); path.closeSubpath(); break;
        case 0xe0bd: path.moveTo(R, T); path.lineTo(L, B); fill = false; break;
        case 0xe0be: path.moveTo(L, T); path.lineTo(R, T); path.lineTo(R, B); path.closeSubpath(); break;
        default: path.moveTo(L, T); path.lineTo(R, B); fill = false; break;
        }
        if (fill)
            p.fillPath(path, white);
        else
            p.strokePath(path, QPen(white, light));
        p.restore();
        return true;
    }
    return false;
}

// A colour faded towards another, for dim (SGR 2) text.
quint32 mix(quint32 a, quint32 b, int pctA) {
    auto ch = [&](int s) { return ((((a >> s) & 0xff) * pctA + ((b >> s) & 0xff) * (100 - pctA)) / 100) & 0xff; };
    return (ch(16) << 16) | (ch(8) << 8) | ch(0);
}

const quint8 UL_SINGLE = 1, UL_DOUBLE = 2, UL_CURLY = 3, UL_DOTTED = 4, UL_DASHED = 5;
const quint8 STRIKE = 64, DIM = 128;
}  // namespace

// ---- the item -----------------------------------------------------------------------------------
GridView::GridView(QQuickItem* parent) : QQuickItem(parent) {
    setFlag(ItemHasContents, true);
    setFlag(ItemAcceptsInputMethod, true);
    setActiveFocusOnTab(true);
    setAcceptedMouseButtons(Qt::LeftButton | Qt::MiddleButton | Qt::RightButton);
    setAcceptHoverEvents(true);
    setCursor(Qt::IBeamCursor);
    g_views.append(this);
    m_blinkTimer.setInterval(530);
    QObject::connect(&m_blinkTimer, &QTimer::timeout, this, [this] {
        m_blinkOn = !m_blinkOn;
        update();
    });
    m_atlas = QImage(ATLAS, ATLAS, QImage::Format_ARGB32_Premultiplied);
    applyFont();
    resetAtlas();
}

namespace {
// The configured font, before the ligature features are set.
QFont lookFont() {
    QFont base(QString::fromStdString(std::string(kyko_palette_font())));
    base.setPointSize(kyko_palette_font_size());
    base.setStyleHint(QFont::Monospace);
    base.setFixedPitch(true);
    base.setKerning(false);
    return base;
}

// A cell: whole pixels, as wide as an M and as tall as the line height makes it.
QSizeF cellOf(const QFontMetricsF& fm) {
    const int pct = std::clamp(kyko_setting(rust::Str("line_height_pct")), 80, 200);
    return QSizeF(std::ceil(fm.horizontalAdvance(QLatin1Char('M'))), std::ceil(fm.lineSpacing() * pct / 100.0));
}
}  // namespace

quint32 grid_cell_px() {
    const QSizeF c = cellOf(QFontMetricsF(lookFont()));
    return (quint32(c.width()) << 16) | (quint32(c.height()) & 0xffff);
}

void GridView::applyFont() {
    QFont base = lookFont();
    m_ligatures = kyko_setting(rust::Str("ligatures")) != 0;
    if (!m_ligatures) {
        // The same features kitty turns off for `disable_ligatures always`.
        base.setFeature(QFont::Tag("liga"), 0);
        base.setFeature(QFont::Tag("calt"), 0);
        base.setFeature(QFont::Tag("dlig"), 0);
    }
    for (int i = 0; i < 4; ++i) {
        m_fonts[i] = base;
        m_fonts[i].setBold(i & 1);
        m_fonts[i].setItalic(i & 2);
    }
    m_font = base;
    QFontMetricsF fm(base);
    const qreal natural = fm.horizontalAdvance(QLatin1Char('M'));
    const QSizeF cell = cellOf(fm);
    m_cellW = cell.width();
    // Narrow glyphs sit in the middle of the whole-pixel cell.
    m_shift = std::floor((m_cellW - natural) / 2);
    m_cellH = cell.height();
    m_ascent = std::round(fm.ascent() + (m_cellH - fm.height()) / 2);
    m_ulThick = std::max<qreal>(1, std::round(fm.lineWidth()));
    m_ulPos = std::min(m_cellH - m_ulThick, std::round(m_ascent + std::max<qreal>(1, fm.underlinePos())));
    m_strikePos = std::round(m_ascent - fm.strikeOutPos());
    m_cursorShape = std::clamp(kyko_setting(rust::Str("cursor")), 0, 2);
    m_blink = kyko_setting(rust::Str("cursor_blink")) != 0;
    m_bgAlpha = uchar(std::clamp(kyko_setting(rust::Str("opacity_pct")), 30, 100) * 255 / 100);
    m_blinkOn = true;
    if (m_blink && hasActiveFocus())
        m_blinkTimer.start();
    else
        m_blinkTimer.stop();
}

void GridView::resetAtlas() {
    m_atlas.fill(Qt::transparent);
    m_tiles.clear();
    m_shelfX = m_shelfY = m_shelfH = 0;
    m_atlasDirty = true;
}

void GridView::reloadLook() {
    applyFont();
    m_shaped.clear();
    m_rawFonts.clear();
    resetAtlas();
    fit();
    update();
}

void GridView::setPane(int pane) {
    if (pane == m_pane)
        return;
    m_pane = pane;
    // The size may be known already: this pane has to hear it.
    m_cols = m_rows = 0;
    fit();
    Q_EMIT paneChanged();
    update();
}

void GridView::fit() {
    // Images are sized in pixels: the pane learns its cell's size.
    const int cw = int(std::round(m_cellW)), ch = int(std::round(m_cellH));
    if (cw != m_sentCellW || ch != m_sentCellH) {
        m_sentCellW = cw;
        m_sentCellH = ch;
        kyko_cell_px(quint16(cw), quint16(ch));
    }
    const int cols = int(width() / m_cellW), rows = int(height() / m_cellH);
    if (cols < 2 || rows < 1 || (cols == m_cols && rows == m_rows))
        return;
    m_cols = cols;
    m_rows = rows;
    if (m_pane > 0)
        kyko_resize(quint32(m_pane), quint16(cols), quint16(rows));
    Q_EMIT gridChanged();
}

GridView::~GridView() {
    g_views.removeAll(this);
    delete m_texture;
    qDeleteAll(m_images);
}

QString GridView::shot() const {
    const auto s = kyko_test_shot();
    return QString::fromUtf8(s.data(), int(s.size()));
}

void GridView::geometryChange(const QRectF& n, const QRectF& o) {
    QQuickItem::geometryChange(n, o);
    fit();
    update();
}

// Images from kitty's graphics protocol: a textured quad each, under the text (z < 0) or over
// it, cut to the item. Textures are kept while an image is in view and made once per version.
void GridView::placeImages(QSGNode* under, QSGNode* over) {
    while (QSGNode* n = under->firstChild()) {
        under->removeChildNode(n);
        delete n;
    }
    while (QSGNode* n = over->firstChild()) {
        over->removeChildNode(n);
        delete n;
    }
    rust::Vec<FfiImage> images;
    kyko_images(quint32(m_pane), images);
    QHash<QPair<quint32, quint64>, QSGTexture*> kept;
    const float w = float(width()), h = float(height());
    for (const FfiImage& im : images) {
        const QPair<quint32, quint64> key(im.image, im.generation);
        QSGTexture* tex = kept.value(key);
        if (!tex)
            tex = m_images.take(key);
        if (!tex) {
            rust::Vec<quint8> px;
            if (im.width == 0 || im.height == 0 || !kyko_image_pixels(quint32(m_pane), im.image, im.generation, px) ||
                px.size() < size_t(im.width) * im.height * 4)
                continue;
            QImage img(px.data(), int(im.width), int(im.height), int(im.width) * 4, QImage::Format_RGBA8888);
            tex = window()->createTextureFromImage(img.copy(), QQuickWindow::TextureHasAlphaChannel);
            if (!tex)
                continue;
            tex->setFiltering(QSGTexture::Linear);
        }
        kept.insert(key, tex);
        if (im.dw == 0 || im.dh == 0 || im.sw == 0 || im.sh == 0)
            continue;
        // Where it goes, then cut to the item, taking the same share off the source.
        float x0 = float(im.col * m_cellW + im.ox), y0 = float(im.row * m_cellH + im.oy);
        float x1 = x0 + float(im.dw), y1 = y0 + float(im.dh);
        float sx0 = float(im.sx), sy0 = float(im.sy), sx1 = sx0 + float(im.sw), sy1 = sy0 + float(im.sh);
        const float kx = (sx1 - sx0) / (x1 - x0), ky = (sy1 - sy0) / (y1 - y0);
        if (x0 < 0) { sx0 -= x0 * kx; x0 = 0; }
        if (y0 < 0) { sy0 -= y0 * ky; y0 = 0; }
        if (x1 > w) { sx1 -= (x1 - w) * kx; x1 = w; }
        if (y1 > h) { sy1 -= (y1 - h) * ky; y1 = h; }
        if (x1 <= x0 || y1 <= y0)
            continue;
        auto* node = new QSGSimpleTextureNode;
        node->setOwnsTexture(false);
        node->setTexture(tex);
        node->setFiltering(QSGTexture::Linear);
        node->setRect(QRectF(x0, y0, x1 - x0, y1 - y0));
        node->setSourceRect(QRectF(sx0, sy0, sx1 - sx0, sy1 - sy0));
        (im.z < 0 ? under : over)->appendChildNode(node);
    }
    // What is no longer in view lets its texture go.
    qDeleteAll(m_images);
    m_images = kept;
}

bool GridView::allocate(int w, int h, int& x, int& y) {
    if (w > ATLAS || h > ATLAS)
        return false;
    if (m_shelfX + w > ATLAS) {
        m_shelfY += m_shelfH;
        m_shelfX = 0;
        m_shelfH = 0;
    }
    if (m_shelfY + h > ATLAS)
        return false;
    x = m_shelfX;
    y = m_shelfY;
    m_shelfX += w;
    m_shelfH = std::max(m_shelfH, h);
    return true;
}

int GridView::fontIndex(const QRawFont& f) {
    for (int i = 0; i < m_rawFonts.size(); ++i)
        if (m_rawFonts[i] == f)
            return i;
    m_rawFonts.append(f);
    return int(m_rawFonts.size()) - 1;
}

GridView::Tile GridView::glyphTile(int font, quint32 glyph) {
    const quint64 key = (quint64(font) << 32) | glyph;
    auto it = m_tiles.constFind(key);
    if (it != m_tiles.constEnd())
        return *it;
    Tile t;
    const QRawFont& rf = m_rawFonts[font];
    const QRectF br = rf.boundingRect(glyph);
    if (br.isEmpty()) {
        m_tiles.insert(key, t);
        return t;
    }
    const int ox = int(std::floor(br.left())) - 1, oy = int(std::floor(br.top())) - 1;
    const int w = int(std::ceil(br.right())) + 1 - ox, h = int(std::ceil(br.bottom())) + 1 - oy;
    int x, y;
    if (!allocate(w, h, x, y)) {
        // The atlas is full: start it again, and draw everything again next frame.
        resetAtlas();
        m_atlasReset = true;
        if (!allocate(w, h, x, y))
            return t;
    }
    {
        QPainter p(&m_atlas);
        p.setCompositionMode(QPainter::CompositionMode_Source);
        p.fillRect(x, y, w, h, Qt::transparent);
        p.setCompositionMode(QPainter::CompositionMode_SourceOver);
        p.setPen(Qt::white);
        QGlyphRun run;
        run.setRawFont(rf);
        run.setGlyphIndexes({glyph});
        run.setPositions({QPointF(0, 0)});
        p.drawGlyphRun(QPointF(x - ox, y - oy), run);
    }
    // A glyph drawn in white has equal red, green, blue and alpha everywhere; anything else is a
    // colour glyph (emoji), drawn with its own colours.
    bool color = false;
    for (int yy = y; yy < y + h && !color; ++yy) {
        const QRgb* line = reinterpret_cast<const QRgb*>(m_atlas.constScanLine(yy));
        for (int xx = x; xx < x + w; ++xx) {
            const QRgb px = line[xx];
            const int a = qAlpha(px);
            if (qRed(px) != a || qGreen(px) != a || qBlue(px) != a) {
                color = true;
                break;
            }
        }
    }
    t = {float(x) / ATLAS, float(y) / ATLAS, float(x + w) / ATLAS, float(y + h) / ATLAS,
         float(ox), float(oy), float(w), float(h), color, true};
    m_tiles.insert(key, t);
    m_atlasDirty = true;
    return t;
}

GridView::Tile GridView::boxTile(quint32 ch) {
    const quint64 key = (quint64(0xffff) << 32) | ch;
    auto it = m_tiles.constFind(key);
    if (it != m_tiles.constEnd())
        return *it;
    Tile t;
    const int w = int(m_cellW), h = int(m_cellH);
    int x, y;
    if (!allocate(w, h, x, y)) {
        resetAtlas();
        m_atlasReset = true;
        if (!allocate(w, h, x, y))
            return t;
    }
    QPainter p(&m_atlas);
    p.setCompositionMode(QPainter::CompositionMode_Source);
    p.fillRect(x, y, w, h, Qt::transparent);
    p.setCompositionMode(QPainter::CompositionMode_SourceOver);
    p.setClipRect(x, y, w, h);
    if (!drawShape(p, ch, x, y, w, h, int(m_ulThick)))
        return t;
    t = {float(x) / ATLAS, float(y) / ATLAS, float(x + w) / ATLAS, float(y + h) / ATLAS,
         0, 0, float(w), float(h), false, true};
    m_tiles.insert(key, t);
    m_atlasDirty = true;
    return t;
}

const QList<GridView::Shaped>& GridView::shape(const QString& text, const QList<int>& cellOf, quint8 style) {
    const QString key = QChar(u'0' + style) + text;
    auto it = m_shaped.constFind(key);
    if (it != m_shaped.constEnd())
        return *it;
    if (m_shaped.size() > 8192)
        m_shaped.clear();
    QList<Shaped> out;
    QTextLayout layout(text, m_fonts[style & 3]);
    QTextOption opt;
    opt.setWrapMode(QTextOption::NoWrap);
    layout.setTextOption(opt);
    layout.setCacheEnabled(true);
    layout.beginLayout();
    QTextLine line = layout.createLine();
    line.setLineWidth(1e7);
    layout.endLayout();
    const qreal base = line.ascent();
    struct G { int font; quint32 glyph; int str; QPointF pos; };
    QList<G> all;
    for (const QGlyphRun& run : layout.glyphRuns(-1, -1, QTextLayout::RetrieveAll)) {
        const int font = fontIndex(run.rawFont());
        const QList<quint32> glyphs = run.glyphIndexes();
        const QList<QPointF> pos = run.positions();
        const QList<qsizetype> str = run.stringIndexes();
        for (qsizetype i = 0; i < glyphs.size(); ++i)
            all.append({font, glyphs[i], int(i < str.size() ? str[i] : 0), pos[i]});
    }
    // Each cell's pen position: the first glyph that belongs to it.
    QHash<int, qreal> penX;
    std::stable_sort(all.begin(), all.end(), [](const G& a, const G& b) { return a.str < b.str; });
    for (const G& g : all) {
        const int cell = cellOf.value(g.str, 0);
        if (!penX.contains(cell))
            penX.insert(cell, g.pos.x());
    }
    for (const G& g : all) {
        const int cell = cellOf.value(g.str, 0);
        out.append({g.font, g.glyph, cell, float(g.pos.x() - penX.value(cell)), float(g.pos.y() - base)});
    }
    return *m_shaped.insert(key, out);
}

void GridView::bell(int mode) {
    if (mode <= 0)
        return;
    if (QQuickWindow* w = window(); w && !w->isActive())
        w->alert(0);
    if (mode == 2) {
        // libcanberra plays the theme's bell through the desktop's sound server; without it
        // the bell is silent rather than a flash nobody asked for.
        QProcess::startDetached(QStringLiteral("canberra-gtk-play"),
                                {QStringLiteral("-i"), QStringLiteral("bell"), QStringLiteral("-d"),
                                 QStringLiteral("kyko")});
        return;
    }
    if (m_flash)
        return;
    m_flash = true;
    update();
    QTimer::singleShot(120, this, [this] {
        m_flash = false;
        update();
    });
}

QSGNode* GridView::updatePaintNode(QSGNode* old, UpdatePaintNodeData*) {
    rust::Vec<FfiCell> cells;
    rust::Vec<FfiExtra> extras;
    rust::Vec<uint8_t> gutter;
    quint16 cols = 0, rows = 0;
    int cx = -1, cy = -1;
    kyko_snapshot(quint32(m_pane), cells, extras, gutter, cols, rows, cx, cy);
    if (cols == 0 || rows == 0 || cells.size() < size_t(cols) * rows || gutter.size() < size_t(rows)) {
        delete old;
        return nullptr;
    }
    m_curX = cx;
    m_curY = cy;
    const bool focused = hasActiveFocus();
    const bool cursorShown = cx >= 0 && (!m_blink || !focused || m_blinkOn);
    // Only a focused block cursor covers its cell; the rest are drawn over the text.
    const bool block = cursorShown && focused && m_cursorShape == 0;

    QSGNode* root = old;
    if (!root) {
        root = new QSGNode;
        root->appendChildNode(makeBgNode());
        root->appendChildNode(new QSGNode);  // images under the text
        auto* gn = new QSGGeometryNode;
        auto* g = new QSGGeometry(glyphAttributes(), 0);
        g->setDrawingMode(QSGGeometry::DrawTriangles);
        g->setVertexDataPattern(QSGGeometry::StreamPattern);
        gn->setGeometry(g);
        gn->setMaterial(new GlyphMaterial);
        gn->setFlags(QSGNode::OwnsGeometry | QSGNode::OwnsMaterial);
        root->appendChildNode(gn);
        root->appendChildNode(makeBgNode());  // decorations: underlines, strikes, other cursors
        root->appendChildNode(new QSGNode);  // images over the text
    }
    const quint32 cursorColor = kyko_palette_cursor();
    const quint32 bgColor = kyko_palette_bg();
    auto* bg = static_cast<QSGGeometryNode*>(root->firstChild());
    fillBg(bg, cells.data(), gutter.data(), cols, rows, m_cellW, m_cellH, width(), height(),
           block ? cx : -1, cy, cursorColor, m_bgAlpha);

    QHash<quint32, QString> combining;
    for (const FfiExtra& e : extras)
        combining.insert(e.index, QString::fromUtf8(e.text.data(), int(e.text.size())));

    // Glyphs: each row in runs of the same style, broken at spaces, drawn shapes and the cursor
    // (so a ligature never hides the character under it), shaped once per distinct run.
    struct Quad { float x0, y0, x1, y1; Tile t; quint32 color; };
    QList<Quad> quads;
    quads.reserve(int(cols) * rows);
    for (int r = 0; r < rows; ++r) {
        const FfiCell* row = cells.data() + size_t(r) * cols;
        const float rowY = float(r * m_cellH), baseY = float(r * m_cellH + m_ascent);
        auto colorOf = [&](int c) {
            const FfiCell& cell = row[c];
            quint32 fg = (cell.flags & DIM) ? mix(cell.fg, cell.bg, 66) : cell.fg;
            if (block && c == cx && r == cy)
                fg = bgColor;
            return fg;
        };
        int c = 0;
        while (c < cols) {
            const FfiCell& first = row[c];
            if (first.ch <= ' ' || (first.flags & WIDE_SPACER)) {
                ++c;
                continue;
            }
            if (drawnShape(first.ch) && !combining.contains(quint32(r * cols + c))) {
                const Tile t = boxTile(first.ch);
                if (t.valid)
                    quads.append({float(c * m_cellW), rowY, float((c + 1) * m_cellW), float(rowY + m_cellH), t, colorOf(c)});
                ++c;
                continue;
            }
            const quint8 style = first.flags & (BOLD | ITALIC);
            QString text;
            QList<int> cellOf;
            int e = c;
            while (e < cols) {
                const FfiCell& cell = row[e];
                if (cell.flags & WIDE_SPACER) {
                    ++e;
                    continue;
                }
                if (e > c && (cell.ch <= ' ' || (cell.flags & (BOLD | ITALIC)) != style || drawnShape(cell.ch)))
                    break;
                // The cursor's cell is a run of its own.
                if (e > c && r == cy && (e == cx || c == cx))
                    break;
                const char32_t u = cell.ch;
                const QString s = QString::fromUcs4(&u, 1) + combining.value(quint32(r * cols + e));
                for (int k = 0; k < s.size(); ++k)
                    cellOf.append(e - c);
                text += s;
                ++e;
            }
            const QList<Shaped>& glyphs = shape(text, cellOf, style);
            for (const Shaped& g : glyphs) {
                const Tile t = glyphTile(g.font, g.glyph);
                if (!t.valid)
                    continue;
                const int col = c + g.cell;
                const float x0 = float(col * m_cellW + m_shift) + g.dx + t.ox;
                const float y0 = baseY + g.dy + t.oy;
                quads.append({x0, y0, x0 + t.w, y0 + t.h, t, colorOf(std::min<int>(col, cols - 1))});
            }
            c = e;
        }
    }

    QSGNode* under = bg->nextSibling();
    auto* gn = static_cast<QSGGeometryNode*>(under->nextSibling());
    QSGGeometry* g = gn->geometry();
    g->allocate(int(quads.size()) * 6);
    auto* v = static_cast<GlyphVertex*>(g->vertexData());
    for (const Quad& q : quads) {
        const uchar R = (q.color >> 16) & 0xff, G = (q.color >> 8) & 0xff, B = q.color & 0xff;
        // Alpha 0 tells the shader to keep the atlas's own colours (an emoji).
        const uchar A = q.t.color ? 0 : 255;
        const Tile& t = q.t;
        v[0] = {q.x0, q.y0, t.u0, t.v0, R, G, B, A};
        v[1] = {q.x1, q.y0, t.u1, t.v0, R, G, B, A};
        v[2] = {q.x0, q.y1, t.u0, t.v1, R, G, B, A};
        v[3] = {q.x1, q.y0, t.u1, t.v0, R, G, B, A};
        v[4] = {q.x1, q.y1, t.u1, t.v1, R, G, B, A};
        v[5] = {q.x0, q.y1, t.u0, t.v1, R, G, B, A};
        v += 6;
    }
    auto* mat = static_cast<GlyphMaterial*>(gn->material());
    if (m_atlasDirty || !m_texture) {
        delete m_texture;
        m_texture = window()->createTextureFromImage(m_atlas, QQuickWindow::TextureHasAlphaChannel);
        m_texture->setFiltering(QSGTexture::Nearest);
        mat->texture = m_texture;
        m_atlasDirty = false;
        gn->markDirty(QSGNode::DirtyMaterial);
    }
    gn->markDirty(QSGNode::DirtyGeometry);

    // Decorations: underlines in their five styles and colours, strikethrough, and the cursor
    // when it is a beam, an underline, or an outline (the window does not have the focus).
    struct Rect { float x0, y0, x1, y1; quint32 c; };
    QList<Rect> deco;
    const float th = float(m_ulThick);
    for (int r = 0; r < rows; ++r) {
        const FfiCell* row = cells.data() + size_t(r) * cols;
        const float top = float(r * m_cellH);
        for (int c = 0; c < cols; ++c) {
            const FfiCell& cell = row[c];
            const float x0 = float(c * m_cellW), x1 = float((c + 1) * m_cellW);
            const quint32 fg = (cell.flags & DIM) ? mix(cell.fg, cell.bg, 66) : cell.fg;
            if (cell.underline) {
                const quint32 uc = cell.underline_color ? (cell.underline_color & 0xffffff) : fg;
                const float y = top + float(m_ulPos);
                switch (cell.underline) {
                case UL_DOUBLE:
                    deco.append({x0, y - th, x1, y, uc});
                    deco.append({x0, y + th, x1, y + 2 * th, uc});
                    break;
                case UL_CURLY: {
                    // A wave of one period per cell, drawn one pixel column at a time.
                    const float amp = std::max(1.0f, th * 1.5f);
                    for (int px = 0; px < int(m_cellW); ++px) {
                        const float phase = float(px) / float(m_cellW) * 6.2831853f;
                        const float yy = y + amp * std::sin(phase);
                        deco.append({x0 + px, std::round(yy) - th / 2, x0 + px + 1, std::round(yy) + th / 2 + (th < 2 ? 1 : 0), uc});
                    }
                    break;
                }
                case UL_DOTTED:
                    for (float px = x0; px < x1; px += 2 * th)
                        deco.append({px, y, std::min(x1, px + th), y + th, uc});
                    break;
                case UL_DASHED: {
                    const float dash = std::max(2.0f, float(m_cellW) / 2 - 1);
                    deco.append({x0, y, x0 + dash, y + th, uc});
                    break;
                }
                default:
                    deco.append({x0, y, x1, y + th, uc});
                }
            }
            if (cell.flags & STRIKE)
                deco.append({x0, top + float(m_strikePos), x1, top + float(m_strikePos) + th, fg});
        }
    }
    for (qsizetype i = 0; i + 2 < m_hover.size(); i += 3) {
        const int r = m_hover[i], a = m_hover[i + 1], b = m_hover[i + 2];
        if (r >= rows)
            continue;
        const quint32 fg = cells[size_t(r) * cols + std::min<int>(a, cols - 1)].fg;
        const float y = float(r * m_cellH + m_ulPos);
        deco.append({float(a * m_cellW), y, float(b * m_cellW), y + th, fg});
    }
    if (cursorShown && !block && cx < cols && cy < rows) {
        const bool wide = cells[size_t(cy) * cols + cx].flags & WIDE;
        const float x0 = float(cx * m_cellW), x1 = float((cx + (wide ? 2 : 1)) * m_cellW);
        const float y0 = float(cy * m_cellH), y1 = float((cy + 1) * m_cellH);
        const float t = std::max(1.0f, th);
        if (!focused) {
            deco.append({x0, y0, x1, y0 + t, cursorColor});
            deco.append({x0, y1 - t, x1, y1, cursorColor});
            deco.append({x0, y0, x0 + t, y1, cursorColor});
            deco.append({x1 - t, y0, x1, y1, cursorColor});
        } else if (m_cursorShape == 1) {
            deco.append({x0, y0, x0 + std::max(2.0f, t), y1, cursorColor});
        } else {
            deco.append({x0, y1 - std::max(2.0f, t), x1, y1, cursorColor});
        }
    }
    // See-through rectangles over the text: the visual bell and the scrollbar.
    struct Veil {
        float x0, y0, x1, y1;
        quint32 c;
        uchar a;
    };
    QList<Veil> veils;
    const float pw = float(width()), ph = float(height());
    const quint32 fgColor = kyko_palette_fg();
    if (m_flash)
        veils.append({0, 0, pw, ph, fgColor, 64});
    kyko_scroll_pos(quint32(m_pane), m_scrollOff, m_scrollHist);
    if (m_scrollHist > 0 && (m_scrollOff > 0 || m_scrollDrag)) {
        const float bw = 6;
        const float total = float(m_scrollHist + quint32(rows));
        const float th = std::max(24.0f, ph * float(rows) / total);
        const float top = (ph - th) * (1.0f - float(m_scrollOff) / float(m_scrollHist));
        veils.append({pw - bw, 0, pw, ph, fgColor, 28});
        veils.append({pw - bw, top, pw, top + th, fgColor, m_scrollDrag ? uchar(200) : uchar(140)});
    }
    auto* dn = static_cast<QSGGeometryNode*>(gn->nextSibling());
    QSGGeometry* dg = dn->geometry();
    dg->allocate(int(deco.size() + veils.size()) * 6);
    auto* dp = dg->vertexDataAsColoredPoint2D();
    for (const Rect& d : deco)
        quad(dp, d.x0, d.y0, d.x1, d.y1, d.c);
    for (const Veil& v : veils) {
        // Premultiplied, as the vertex colour material expects.
        const uchar r = uchar(((v.c >> 16) & 0xff) * v.a / 255), g = uchar(((v.c >> 8) & 0xff) * v.a / 255),
                    b = uchar((v.c & 0xff) * v.a / 255);
        dp[0].set(v.x0, v.y0, r, g, b, v.a);
        dp[1].set(v.x1, v.y0, r, g, b, v.a);
        dp[2].set(v.x0, v.y1, r, g, b, v.a);
        dp[3].set(v.x1, v.y0, r, g, b, v.a);
        dp[4].set(v.x1, v.y1, r, g, b, v.a);
        dp[5].set(v.x0, v.y1, r, g, b, v.a);
        dp += 6;
    }
    dn->markDirty(QSGNode::DirtyGeometry);
    placeImages(under, dn->nextSibling());

    if (m_atlasReset) {
        m_atlasReset = false;
        update();
    }
    return root;
}

void GridView::sendMouse(quint8 kind, quint8 button, const QPointF& pos, Qt::KeyboardModifiers mods, quint8 clicks) {
    if (m_pane <= 0 || m_cols == 0 || m_rows == 0)
        return;
    const int col = std::clamp(int(pos.x() / m_cellW), 0, m_cols - 1);
    const int row = std::clamp(int(pos.y() / m_cellH), 0, m_rows - 1);
    const bool right = (pos.x() - col * m_cellW) * 2 >= m_cellW;
    quint8 m = 0;
    if (mods & Qt::ShiftModifier) m |= 1;
    if (mods & Qt::AltModifier) m |= 2;
    if (mods & Qt::ControlModifier) m |= 4;
    kyko_mouse(quint32(m_pane), kind, button, quint16(col), quint16(row), right, m, clicks);
}

static quint8 buttonCode(Qt::MouseButton b) {
    return b == Qt::LeftButton ? 0 : b == Qt::MiddleButton ? 1 : b == Qt::RightButton ? 2 : 3;
}

void GridView::scrollToY(qreal y) {
    const qreal f = std::clamp(y / std::max<qreal>(1, height()), 0.0, 1.0);
    kyko_scroll_to(quint32(m_pane), quint32(std::lround(qreal(m_scrollHist) * (1.0 - f))));
    update();
}

void GridView::mousePressEvent(QMouseEvent* e) {
    forceActiveFocus();
    if (e->button() == Qt::LeftButton && m_scrollOff > 0 && m_scrollHist > 0 && e->position().x() >= width() - 10) {
        m_scrollDrag = true;
        scrollToY(e->position().y());
        e->accept();
        return;
    }
    const QPoint cell(int(e->position().x() / m_cellW), int(e->position().y() / m_cellH));
    const qint64 now = e->timestamp();
    m_clicks = (now - m_lastPress < 400 && cell == m_lastCell && m_clicks < 3) ? m_clicks + 1 : 1;
    m_lastPress = now;
    m_lastCell = cell;
    m_dragged = false;
    m_down |= e->button();
    if (e->button() == Qt::MiddleButton && m_pane > 0) {
        const quint8 m = (e->modifiers() & Qt::ShiftModifier) ? 1 : 0;
        QClipboard* c = QGuiApplication::clipboard();
        if (c->supportsSelection() && kyko_middle_paste(quint32(m_pane), m)) {
            const std::string t = c->text(QClipboard::Selection).toStdString();
            if (!t.empty())
                kyko_paste(quint32(m_pane), rust::Str(t.data(), t.size()));
            e->accept();
            return;
        }
    }
    sendMouse(0, buttonCode(e->button()), e->position(), e->modifiers(), quint8(m_clicks));
    e->accept();
}

void GridView::mouseMoveEvent(QMouseEvent* e) {
    if (m_scrollDrag) {
        scrollToY(e->position().y());
        e->accept();
        return;
    }
    if (QPoint(int(e->position().x() / m_cellW), int(e->position().y() / m_cellH)) != m_lastCell)
        m_dragged = true;
    const Qt::MouseButton b = (m_down & Qt::LeftButton) ? Qt::LeftButton : (m_down & Qt::MiddleButton) ? Qt::MiddleButton : Qt::RightButton;
    sendMouse(1, buttonCode(b), e->position(), e->modifiers(), 0);
    e->accept();
}

void GridView::mouseReleaseEvent(QMouseEvent* e) {
    if (m_scrollDrag && e->button() == Qt::LeftButton) {
        m_scrollDrag = false;
        update();
        e->accept();
        return;
    }
    m_down &= ~Qt::MouseButtons(e->button());
    sendMouse(2, buttonCode(e->button()), e->position(), e->modifiers(), 0);
    if (e->button() == Qt::LeftButton && !m_dragged && m_clicks == 1 && m_pane > 0 && m_cols > 0) {
        const int col = std::clamp(int(e->position().x() / m_cellW), 0, m_cols - 1);
        const int row = std::clamp(int(e->position().y() / m_cellH), 0, m_rows - 1);
        quint8 m = 0;
        if (e->modifiers() & Qt::ShiftModifier) m |= 1;
        if (e->modifiers() & Qt::AltModifier) m |= 2;
        if (e->modifiers() & Qt::ControlModifier) m |= 4;
        kyko_open_link(quint32(m_pane), quint16(col), quint16(row), m);
    }
    e->accept();
}

void GridView::hoverLink(const QPointF& pos) {
    QList<quint16> spans;
    if (m_pane > 0 && m_cols > 0 && pos.x() >= 0 && pos.y() >= 0) {
        const int col = int(pos.x() / m_cellW), row = int(pos.y() / m_cellH);
        rust::Vec<quint16> out;
        if (col < m_cols && row < m_rows && kyko_link_spans(quint32(m_pane), quint16(col), quint16(row), out))
            for (quint16 v : out)
                spans.append(v);
    }
    if (spans == m_hover)
        return;
    m_hover = spans;
    setCursor(m_hover.isEmpty() ? Qt::IBeamCursor : Qt::PointingHandCursor);
    update();
}

void GridView::hoverMoveEvent(QHoverEvent* e) {
    hoverLink(e->position());
    // Only programs asking for all motion hear this; Rust drops it for the rest.
    if (m_down == Qt::NoButton)
        sendMouse(1, 3, e->position(), e->modifiers(), 0);
}

void GridView::wheelEvent(QWheelEvent* e) {
    // Whole notches: a touchpad's small steps add up until they make one.
    m_wheel += e->angleDelta().y();
    while (m_wheel >= 120) {
        sendMouse(3, 0, e->position(), e->modifiers(), 0);
        m_wheel -= 120;
    }
    while (m_wheel <= -120) {
        sendMouse(3, 1, e->position(), e->modifiers(), 0);
        m_wheel += 120;
    }
    e->accept();
}

void GridView::focusInEvent(QFocusEvent* e) {
    QQuickItem::focusInEvent(e);
    if (m_pane > 0)
        kyko_focus_report(quint32(m_pane), true);
    m_blinkOn = true;
    if (m_blink)
        m_blinkTimer.start();
    update();
}

void GridView::focusOutEvent(QFocusEvent* e) {
    QQuickItem::focusOutEvent(e);
    if (m_pane > 0)
        kyko_focus_report(quint32(m_pane), false);
    m_blinkTimer.stop();
    m_blinkOn = true;
    update();
}

void GridView::keyPressEvent(QKeyEvent* e) {
    // Typing shows the cursor at once and starts its blink again.
    if (m_blink) {
        m_blinkOn = true;
        m_blinkTimer.start();
    }
    quint8 mods = 0;
    if (e->modifiers() & Qt::ShiftModifier)
        mods |= 1;
    if (e->modifiers() & Qt::AltModifier)
        mods |= 2;
    if (e->modifiers() & Qt::ControlModifier)
        mods |= 4;
    if (e->modifiers() & Qt::MetaModifier)
        mods |= 8;
    const std::string text = e->text().toStdString();
    const quint8 event = e->isAutoRepeat() ? 2 : 1;
    if (kyko_key(quint32(m_pane), quint32(e->key()), mods, rust::Str(text.data(), text.size()), event))
        e->accept();
    else
        e->ignore();
}

void GridView::keyReleaseEvent(QKeyEvent* e) {
    if (e->isAutoRepeat()) {
        e->accept();
        return;
    }
    quint8 mods = 0;
    if (e->modifiers() & Qt::ShiftModifier)
        mods |= 1;
    if (e->modifiers() & Qt::AltModifier)
        mods |= 2;
    if (e->modifiers() & Qt::ControlModifier)
        mods |= 4;
    if (e->modifiers() & Qt::MetaModifier)
        mods |= 8;
    const std::string text = e->text().toStdString();
    if (kyko_key(quint32(m_pane), quint32(e->key()), mods, rust::Str(text.data(), text.size()), 3))
        e->accept();
    else
        e->ignore();
}

void GridView::inputMethodEvent(QInputMethodEvent* e) {
    const std::string text = e->commitString().toStdString();
    if (!text.empty())
        kyko_text(quint32(m_pane), rust::Str(text.data(), text.size()));
    e->accept();
}

QVariant GridView::inputMethodQuery(Qt::InputMethodQuery query) const {
    switch (query) {
    case Qt::ImEnabled: return true;
    case Qt::ImCursorRectangle:
        return QRectF(std::max(0, m_curX) * m_cellW, std::max(0, m_curY) * m_cellH, m_cellW, m_cellH);
    case Qt::ImFont: return m_font;
    default: return QQuickItem::inputMethodQuery(query);
    }
}

void app_set_translucent() {
    QQuickWindow::setDefaultAlphaBuffer(true);
}

void app_set_id(rust::Str id) {
    QGuiApplication::setDesktopFileName(QString::fromUtf8(id.data(), int(id.size())));
}
