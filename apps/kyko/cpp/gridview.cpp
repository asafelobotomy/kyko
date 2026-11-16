#include "gridview.h"

#include <QtCore/QPointer>
#include <QtGui/QFontMetricsF>
#include <QtGui/QGuiApplication>
#include <QtGui/QInputMethodEvent>
#include <QtGui/QKeyEvent>
#include <QtGui/QPainter>
#include <QtQml/qqml.h>
#include <QtQuick/QQuickWindow>
#include <QtQuick/QSGGeometryNode>
#include <QtQuick/QSGMaterial>
#include <QtQuick/QSGMaterialShader>
#include <QtQuick/QSGTexture>
#include <QtQuick/QSGVertexColorMaterial>
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
void fillBg(QSGGeometryNode* node, const FfiCell* cells, int cols, int rows, qreal cw, qreal ch, qreal w, qreal h,
            int curX, int curY, quint32 curColor) {
    const quint32 def = kyko_palette_bg();
    int quads = 1 + (curX >= 0 ? 1 : 0);
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
    quad(p, 0, 0, float(w), float(h), def);
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

// ---- the item -----------------------------------------------------------------------------------
GridView::GridView(QQuickItem* parent) : QQuickItem(parent) {
    setFlag(ItemHasContents, true);
    setFlag(ItemAcceptsInputMethod, true);
    setActiveFocusOnTab(true);
    g_views.append(this);
    applyFont();
    m_atlas = QImage(ATLAS, ATLAS, QImage::Format_ARGB32_Premultiplied);
    m_atlas.fill(Qt::transparent);
}

void GridView::applyFont() {
    m_font = QFont(QString::fromStdString(std::string(kyko_palette_font())));
    m_font.setPointSize(kyko_palette_font_size());
    m_font.setStyleHint(QFont::Monospace);
    m_font.setFixedPitch(true);
    m_font.setKerning(false);
    QFontMetricsF fm(m_font);
    const qreal natural = fm.horizontalAdvance(QLatin1Char('M'));
    m_cellW = std::ceil(natural);
    // Whole-pixel advances keep every cell on the pixel grid.
    m_font.setLetterSpacing(QFont::AbsoluteSpacing, m_cellW - natural);
    m_cellH = std::ceil(fm.lineSpacing());
    m_ascent = fm.ascent() + (m_cellH - fm.height()) / 2;
}

void GridView::reloadLook() {
    applyFont();
    m_slots.clear();
    m_nextSlot = 0;
    m_atlas.fill(Qt::transparent);
    m_atlasDirty = true;
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

GridView::Slot GridView::slotFor(quint32 ch, quint8 style) {
    const quint64 key = quint64(ch) | (quint64(style) << 32);
    auto it = m_slots.constFind(key);
    if (it != m_slots.constEnd())
        return *it;
    const int cw = int(m_cellW), chh = int(m_cellH);
    const int perRow = ATLAS / cw;
    const char32_t u = ch;
    const QString s = QString::fromUcs4(&u, 1);
    const bool wide = style & WIDE;
    const int span = wide ? 2 : 1;
    if (m_nextSlot % perRow + span > perRow)
        m_nextSlot += perRow - m_nextSlot % perRow;
    const int sx = (m_nextSlot % perRow) * cw, sy = (m_nextSlot / perRow) * chh;
    m_nextSlot += span;
    Slot slot{0, 0, 0, 0, wide};
    if (sy + chh <= ATLAS) {
        QPainter p(&m_atlas);
        QFont f = m_font;
        f.setBold(style & BOLD);
        f.setItalic(style & ITALIC);
        f.setUnderline(style & UNDERLINE);
        p.setFont(f);
        p.setPen(Qt::white);
        p.setClipRect(sx, sy, cw * span, chh);
        p.drawText(QPointF(sx, sy + m_ascent), s);
        p.end();
        slot = {float(sx) / ATLAS, float(sy) / ATLAS, float(sx + cw * span) / ATLAS, float(sy + chh) / ATLAS, wide};
        m_atlasDirty = true;
    }
    m_slots.insert(key, slot);
    return slot;
}

QSGNode* GridView::updatePaintNode(QSGNode* old, UpdatePaintNodeData*) {
    rust::Vec<FfiCell> cells;
    quint16 cols = 0, rows = 0;
    int cx = -1, cy = -1;
    kyko_snapshot(quint32(m_pane), cells, cols, rows, cx, cy);
    if (cols == 0 || rows == 0 || cells.size() < size_t(cols) * rows) {
        delete old;
        return nullptr;
    }
    m_curX = cx;
    m_curY = cy;

    QSGNode* root = old;
    if (!root) {
        root = new QSGNode;
        root->appendChildNode(makeBgNode());
        auto* gn = new QSGGeometryNode;
        auto* g = new QSGGeometry(glyphAttributes(), 0);
        g->setDrawingMode(QSGGeometry::DrawTriangles);
        g->setVertexDataPattern(QSGGeometry::StreamPattern);
        gn->setGeometry(g);
        gn->setMaterial(new GlyphMaterial);
        gn->setFlags(QSGNode::OwnsGeometry | QSGNode::OwnsMaterial);
        root->appendChildNode(gn);
    }
    const quint32 cursorColor = kyko_palette_cursor();
    auto* bg = static_cast<QSGGeometryNode*>(root->firstChild());
    fillBg(bg, cells.data(), cols, rows, m_cellW, m_cellH, width(), height(), cx, cy, cursorColor);

    auto* gn = static_cast<QSGGeometryNode*>(bg->nextSibling());
    int count = 0;
    for (const FfiCell& c : cells)
        if (c.ch > ' ' && !(c.flags & WIDE_SPACER))
            ++count;
    QSGGeometry* g = gn->geometry();
    g->allocate(count * 6);
    auto* v = static_cast<GlyphVertex*>(g->vertexData());
    for (int r = 0; r < rows; ++r) {
        const FfiCell* row = cells.data() + size_t(r) * cols;
        const float y0 = float(r * m_cellH), y1 = float((r + 1) * m_cellH);
        for (int c = 0; c < cols; ++c) {
            const FfiCell& cell = row[c];
            if (cell.ch <= ' ' || (cell.flags & WIDE_SPACER))
                continue;
            const Slot s = slotFor(cell.ch, cell.flags & (BOLD | ITALIC | UNDERLINE | WIDE));
            const float x0 = float(c * m_cellW), x1 = float((c + (s.wide ? 2 : 1)) * m_cellW);
            // Under the cursor block the glyph takes the background colour.
            const quint32 fg = (c == cx && r == cy) ? kyko_palette_bg() : cell.fg;
            const uchar R = (fg >> 16) & 0xff, G = (fg >> 8) & 0xff, B = fg & 0xff;
            v[0] = {x0, y0, s.u0, s.v0, R, G, B, 255};
            v[1] = {x1, y0, s.u1, s.v0, R, G, B, 255};
            v[2] = {x0, y1, s.u0, s.v1, R, G, B, 255};
            v[3] = {x1, y0, s.u1, s.v0, R, G, B, 255};
            v[4] = {x1, y1, s.u1, s.v1, R, G, B, 255};
            v[5] = {x0, y1, s.u0, s.v1, R, G, B, 255};
            v += 6;
        }
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
    return root;
}

void GridView::focusInEvent(QFocusEvent* e) {
    QQuickItem::focusInEvent(e);
    update();
}

void GridView::keyPressEvent(QKeyEvent* e) {
    quint8 mods = 0;
    if (e->modifiers() & Qt::ShiftModifier)
        mods |= 1;
    if (e->modifiers() & Qt::AltModifier)
        mods |= 2;
    if (e->modifiers() & Qt::ControlModifier)
        mods |= 4;
    const std::string text = e->text().toStdString();
    if (kyko_key(quint32(m_pane), quint32(e->key()), mods, rust::Str(text.data(), text.size())))
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

void app_set_id(rust::Str id) {
    QGuiApplication::setDesktopFileName(QString::fromUtf8(id.data(), int(id.size())));
}
