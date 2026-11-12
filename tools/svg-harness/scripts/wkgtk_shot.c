#include <gtk/gtk.h>
#include <webkit2/webkit2.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static void fail(const char *message)
{
    fprintf(stderr, "%s\n", message);
    exit(EXIT_FAILURE);
}

static void snapshot_ready(GObject *object, GAsyncResult *result, gpointer output)
{
    GError *error = NULL;
    cairo_surface_t *surface = webkit_web_view_get_snapshot_finish(WEBKIT_WEB_VIEW(object), result, &error);
    if (!surface)
        fail(error ? error->message : "WebKitGTK snapshot failed");
    cairo_status_t status = cairo_surface_write_to_png(surface, output);
    cairo_surface_destroy(surface);
    if (status != CAIRO_STATUS_SUCCESS)
        fail(cairo_status_to_string(status));
    gtk_main_quit();
}

static void loaded(WebKitWebView *view, WebKitLoadEvent event, gpointer output)
{
    if (event == WEBKIT_LOAD_FINISHED)
        webkit_web_view_get_snapshot(view, WEBKIT_SNAPSHOT_REGION_VISIBLE, WEBKIT_SNAPSHOT_OPTIONS_NONE,
                                     NULL, snapshot_ready, output);
}

static gboolean load_failed(WebKitWebView *view, WebKitLoadEvent event, const char *uri, GError *error,
                            gpointer data)
{
    fail(error->message);
    return TRUE;
}

int main(int argc, char **argv)
{
    if (argc == 2 && strcmp(argv[1], "--version") == 0) {
        printf("%u.%u.%u\n", webkit_get_major_version(), webkit_get_minor_version(), webkit_get_micro_version());
        return EXIT_SUCCESS;
    }
    if (argc != 5)
        fail("usage: wkgtk_shot page.html out.png width height");
    if (!gtk_init_check(NULL, NULL))
        fail("WebKitGTK needs a display. On headless Linux, run refs.py under xvfb-run.");

    GtkWidget *window = gtk_window_new(GTK_WINDOW_TOPLEVEL);
    gtk_window_set_decorated(GTK_WINDOW(window), FALSE);
    gtk_window_set_default_size(GTK_WINDOW(window), atoi(argv[3]), atoi(argv[4]));
    WebKitWebContext *context = webkit_web_context_new_ephemeral();
    GtkWidget *view = webkit_web_view_new_with_context(context);
    g_object_unref(context);
    gtk_container_add(GTK_CONTAINER(window), view);
    g_signal_connect(view, "load-changed", G_CALLBACK(loaded), argv[2]);
    g_signal_connect(view, "load-failed", G_CALLBACK(load_failed), NULL);
    gtk_widget_show_all(window);

    GFile *page = g_file_new_for_commandline_arg(argv[1]);
    char *uri = g_file_get_uri(page);
    webkit_web_view_load_uri(WEBKIT_WEB_VIEW(view), uri);
    g_free(uri);
    g_object_unref(page);
    gtk_main();
    return EXIT_SUCCESS;
}
