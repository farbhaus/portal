--
-- PostgreSQL database dump
--

\restrict 2xpzO2r1JI8r3NRRfZhGYfYqRiN9harcVtK4cDLoEO6x60emVyKJxbjkCvJBdTe

-- Dumped from database version 16.15 (Debian 16.15-1.pgdg13+2)
-- Dumped by pg_dump version 16.15 (Debian 16.15-1.pgdg13+2)

SET statement_timeout = 0;
SET lock_timeout = 0;
SET idle_in_transaction_session_timeout = 0;
SET client_encoding = 'UTF8';
SET standard_conforming_strings = on;
SELECT pg_catalog.set_config('search_path', '', false);
SET check_function_bodies = false;
SET xmloption = content;
SET client_min_messages = warning;
SET row_security = off;

SET default_tablespace = '';

SET default_table_access_method = heap;

--
-- Name: alembic_version; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.alembic_version (
    version_num character varying(32) NOT NULL
);


--
-- Name: app_settings; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.app_settings (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    logo_png bytea,
    logo_content_type character varying(64),
    logo_updated_at timestamp with time zone,
    brand_display_name character varying(255),
    brand_accent_color character varying(32),
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    smtp_host character varying(255),
    smtp_port integer,
    smtp_username character varying(255),
    smtp_password_encrypted text,
    smtp_from character varying(255),
    smtp_use_tls boolean,
    smtp_starttls boolean,
    notify_email character varying(255),
    frameio_client_id character varying(255),
    frameio_client_secret_encrypted text,
    config_seeded boolean DEFAULT false NOT NULL
);


--
-- Name: audit_log; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.audit_log (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    user_id uuid,
    action character varying(128) NOT NULL,
    detail jsonb,
    created_at timestamp with time zone DEFAULT now() NOT NULL
);


--
-- Name: download_events; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.download_events (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    download_session_id uuid NOT NULL,
    frameio_file_id character varying(255) NOT NULL,
    file_name character varying(512),
    completed boolean NOT NULL,
    bytes_served bigint,
    started_at timestamp with time zone DEFAULT now() NOT NULL,
    ip character varying(64),
    user_agent text
);


--
-- Name: download_sessions; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.download_sessions (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    download_link_id uuid NOT NULL,
    viewer_name character varying(255),
    viewer_email character varying(255),
    ip character varying(64),
    user_agent text,
    started_at timestamp with time zone,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    files jsonb
);


--
-- Name: email_verifications; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.email_verifications (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    email character varying(255) NOT NULL,
    code_hash character varying(64) NOT NULL,
    expires_at timestamp with time zone NOT NULL,
    attempts integer NOT NULL,
    last_sent_at timestamp with time zone DEFAULT now() NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL
);


--
-- Name: frameio_connections; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.frameio_connections (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    user_id uuid NOT NULL,
    adobe_ims_user_id character varying(255),
    adobe_email character varying(255),
    access_token_encrypted text,
    refresh_token_encrypted text,
    token_expires_at timestamp with time zone,
    workspaces jsonb,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL
);


--
-- Name: link_presets; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.link_presets (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    name character varying(255) NOT NULL,
    kind character varying(16) NOT NULL,
    settings jsonb DEFAULT '{}'::jsonb NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT link_presets_kind_check CHECK (((kind)::text = ANY ((ARRAY['upload'::character varying, 'download'::character varying])::text[])))
);


--
-- Name: links; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.links (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    kind character varying(16) NOT NULL,
    token character varying(64) NOT NULL,
    name character varying(255),
    expires_at timestamp with time zone,
    password_hash character varying(255),
    revoked_at timestamp with time zone,
    verify_email boolean DEFAULT false NOT NULL,
    brand_display_name character varying(255),
    brand_subtitle text,
    notify_emails jsonb,
    remind_before_expiry boolean DEFAULT false NOT NULL,
    expiry_reminded_at timestamp with time zone,
    folder jsonb,
    max_file_size bigint,
    allowed_extensions jsonb,
    uploader_fields_required jsonb,
    target_folder_id character varying(255),
    target_folder_name character varying(255),
    subfolder_template text,
    source jsonb,
    max_downloads integer,
    downloads_count integer DEFAULT 0 NOT NULL,
    viewer_fields_required jsonb,
    allow_preview boolean DEFAULT true NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    return_for uuid,
    CONSTRAINT links_download_source_check CHECK ((((kind)::text <> 'download'::text) OR (source IS NOT NULL))),
    CONSTRAINT links_kind_check CHECK (((kind)::text = ANY ((ARRAY['upload'::character varying, 'download'::character varying])::text[])))
);


--
-- Name: upload_sessions; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.upload_sessions (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    upload_link_id uuid NOT NULL,
    uploader_name character varying(255),
    uploader_email character varying(255),
    uploader_message text,
    status character varying(32) NOT NULL,
    frameio_asset_ids jsonb,
    total_bytes bigint,
    started_at timestamp with time zone,
    completed_at timestamp with time zone,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    uploaded_bytes bigint,
    last_activity_at timestamp with time zone
);


--
-- Name: users; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.users (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    email character varying(255) NOT NULL,
    password_hash character varying(255) NOT NULL,
    is_active boolean NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    totp_secret_encrypted text,
    totp_enabled boolean DEFAULT false NOT NULL,
    totp_recovery_codes jsonb
);


--
-- Name: webauthn_credentials; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.webauthn_credentials (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    user_id uuid NOT NULL,
    credential_id character varying(512) NOT NULL,
    public_key text NOT NULL,
    sign_count bigint NOT NULL,
    transports jsonb,
    name character varying(255),
    last_used_at timestamp with time zone,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL
);


--
-- Name: alembic_version alembic_version_pkc; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.alembic_version
    ADD CONSTRAINT alembic_version_pkc PRIMARY KEY (version_num);


--
-- Name: app_settings app_settings_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.app_settings
    ADD CONSTRAINT app_settings_pkey PRIMARY KEY (id);


--
-- Name: audit_log audit_log_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.audit_log
    ADD CONSTRAINT audit_log_pkey PRIMARY KEY (id);


--
-- Name: download_events download_events_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.download_events
    ADD CONSTRAINT download_events_pkey PRIMARY KEY (id);


--
-- Name: download_sessions download_sessions_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.download_sessions
    ADD CONSTRAINT download_sessions_pkey PRIMARY KEY (id);


--
-- Name: email_verifications email_verifications_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.email_verifications
    ADD CONSTRAINT email_verifications_pkey PRIMARY KEY (id);


--
-- Name: frameio_connections frameio_connections_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.frameio_connections
    ADD CONSTRAINT frameio_connections_pkey PRIMARY KEY (id);


--
-- Name: link_presets link_presets_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.link_presets
    ADD CONSTRAINT link_presets_pkey PRIMARY KEY (id);


--
-- Name: links links_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.links
    ADD CONSTRAINT links_pkey PRIMARY KEY (id);


--
-- Name: upload_sessions upload_sessions_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.upload_sessions
    ADD CONSTRAINT upload_sessions_pkey PRIMARY KEY (id);


--
-- Name: users users_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.users
    ADD CONSTRAINT users_pkey PRIMARY KEY (id);


--
-- Name: webauthn_credentials webauthn_credentials_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.webauthn_credentials
    ADD CONSTRAINT webauthn_credentials_pkey PRIMARY KEY (id);


--
-- Name: ix_audit_log_user_id; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX ix_audit_log_user_id ON public.audit_log USING btree (user_id);


--
-- Name: ix_download_events_download_session_id; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX ix_download_events_download_session_id ON public.download_events USING btree (download_session_id);


--
-- Name: ix_download_events_frameio_file_id; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX ix_download_events_frameio_file_id ON public.download_events USING btree (frameio_file_id);


--
-- Name: ix_download_sessions_download_link_id; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX ix_download_sessions_download_link_id ON public.download_sessions USING btree (download_link_id);


--
-- Name: ix_email_verifications_email; Type: INDEX; Schema: public; Owner: -
--

CREATE UNIQUE INDEX ix_email_verifications_email ON public.email_verifications USING btree (email);


--
-- Name: ix_frameio_connections_user_id; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX ix_frameio_connections_user_id ON public.frameio_connections USING btree (user_id);


--
-- Name: ix_links_return_for; Type: INDEX; Schema: public; Owner: -
--

CREATE UNIQUE INDEX ix_links_return_for ON public.links USING btree (return_for);


--
-- Name: ix_links_token; Type: INDEX; Schema: public; Owner: -
--

CREATE UNIQUE INDEX ix_links_token ON public.links USING btree (token);


--
-- Name: ix_upload_sessions_upload_link_id; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX ix_upload_sessions_upload_link_id ON public.upload_sessions USING btree (upload_link_id);


--
-- Name: ix_users_email; Type: INDEX; Schema: public; Owner: -
--

CREATE UNIQUE INDEX ix_users_email ON public.users USING btree (email);


--
-- Name: ix_webauthn_credentials_credential_id; Type: INDEX; Schema: public; Owner: -
--

CREATE UNIQUE INDEX ix_webauthn_credentials_credential_id ON public.webauthn_credentials USING btree (credential_id);


--
-- Name: ix_webauthn_credentials_user_id; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX ix_webauthn_credentials_user_id ON public.webauthn_credentials USING btree (user_id);


--
-- Name: audit_log audit_log_user_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.audit_log
    ADD CONSTRAINT audit_log_user_id_fkey FOREIGN KEY (user_id) REFERENCES public.users(id) ON DELETE SET NULL;


--
-- Name: download_events download_events_download_session_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.download_events
    ADD CONSTRAINT download_events_download_session_id_fkey FOREIGN KEY (download_session_id) REFERENCES public.download_sessions(id) ON DELETE CASCADE;


--
-- Name: download_sessions download_sessions_download_link_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.download_sessions
    ADD CONSTRAINT download_sessions_download_link_id_fkey FOREIGN KEY (download_link_id) REFERENCES public.links(id) ON DELETE CASCADE;


--
-- Name: frameio_connections frameio_connections_user_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.frameio_connections
    ADD CONSTRAINT frameio_connections_user_id_fkey FOREIGN KEY (user_id) REFERENCES public.users(id) ON DELETE CASCADE;


--
-- Name: links links_return_for_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.links
    ADD CONSTRAINT links_return_for_fkey FOREIGN KEY (return_for) REFERENCES public.links(id) ON DELETE CASCADE;


--
-- Name: upload_sessions upload_sessions_upload_link_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.upload_sessions
    ADD CONSTRAINT upload_sessions_upload_link_id_fkey FOREIGN KEY (upload_link_id) REFERENCES public.links(id) ON DELETE CASCADE;


--
-- Name: webauthn_credentials webauthn_credentials_user_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.webauthn_credentials
    ADD CONSTRAINT webauthn_credentials_user_id_fkey FOREIGN KEY (user_id) REFERENCES public.users(id) ON DELETE CASCADE;


--
-- PostgreSQL database dump complete
--

\unrestrict 2xpzO2r1JI8r3NRRfZhGYfYqRiN9harcVtK4cDLoEO6x60emVyKJxbjkCvJBdTe


--
-- Schema revision this file corresponds to. A database created from it is stamped with it;
-- portal-api migrate upgrades older revisions with migrations/ and refuses unknown ones
-- (src/migrate.rs). Generated by tools/schema_check.sh --write; CI fails if it drifts.
--
INSERT INTO public.alembic_version (version_num) VALUES ('b4c5d6e7f8a9');
