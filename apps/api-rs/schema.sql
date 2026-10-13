--
-- PostgreSQL database dump
--

\restrict tfbENkYsaCXcqiOD94YqXLoWlNWqg8vP3gLXERIJj6Hihj7SNxiEVvfqqtIgmAz

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
-- Name: destinations; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.destinations (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    display_name character varying(255) NOT NULL,
    config jsonb NOT NULL,
    subtitle text,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL
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
-- Name: download_links; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.download_links (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    token character varying(64) NOT NULL,
    source jsonb NOT NULL,
    expires_at timestamp with time zone,
    password_hash character varying(255),
    max_downloads integer,
    downloads_count integer NOT NULL,
    viewer_fields_required jsonb,
    revoked_at timestamp with time zone,
    allow_preview boolean NOT NULL,
    brand_display_name character varying(255),
    brand_subtitle text,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    verify_email boolean DEFAULT false NOT NULL
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
-- Name: sync_jobs; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.sync_jobs (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    sync_rule_id uuid NOT NULL,
    frameio_file_id character varying(255) NOT NULL,
    status character varying(32) NOT NULL,
    bytes bigint,
    sha256 character varying(64),
    error text,
    retry_count integer NOT NULL,
    started_at timestamp with time zone,
    completed_at timestamp with time zone,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    source_wait_started_at timestamp with time zone
);


--
-- Name: sync_rules; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.sync_rules (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    name character varying(255) NOT NULL,
    source_config jsonb NOT NULL,
    destination_path text NOT NULL,
    conflict_policy character varying(32) NOT NULL,
    path_template text,
    enabled boolean NOT NULL,
    webhook_secret text,
    frameio_webhook_id character varying(255),
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL
);


--
-- Name: upload_links; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.upload_links (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    token character varying(64) NOT NULL,
    destination_id uuid NOT NULL,
    expires_at timestamp with time zone,
    password_hash character varying(255),
    max_file_size bigint,
    allowed_extensions jsonb,
    uploader_fields_required jsonb,
    revoked_at timestamp with time zone,
    brand_display_name character varying(255),
    brand_subtitle text,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    verify_email boolean DEFAULT false NOT NULL,
    target_folder_id character varying(255),
    target_folder_name character varying(255),
    subfolder_template text
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
-- Name: webhook_events; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.webhook_events (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    source character varying(32) NOT NULL,
    event_type character varying(128),
    raw jsonb NOT NULL,
    signature_valid boolean,
    processed boolean NOT NULL,
    processed_at timestamp with time zone,
    error text,
    received_at timestamp with time zone DEFAULT now() NOT NULL
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
-- Name: destinations destinations_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.destinations
    ADD CONSTRAINT destinations_pkey PRIMARY KEY (id);


--
-- Name: download_events download_events_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.download_events
    ADD CONSTRAINT download_events_pkey PRIMARY KEY (id);


--
-- Name: download_links download_links_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.download_links
    ADD CONSTRAINT download_links_pkey PRIMARY KEY (id);


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
-- Name: sync_jobs sync_jobs_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.sync_jobs
    ADD CONSTRAINT sync_jobs_pkey PRIMARY KEY (id);


--
-- Name: sync_rules sync_rules_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.sync_rules
    ADD CONSTRAINT sync_rules_pkey PRIMARY KEY (id);


--
-- Name: upload_links upload_links_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.upload_links
    ADD CONSTRAINT upload_links_pkey PRIMARY KEY (id);


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
-- Name: webhook_events webhook_events_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.webhook_events
    ADD CONSTRAINT webhook_events_pkey PRIMARY KEY (id);


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
-- Name: ix_download_links_token; Type: INDEX; Schema: public; Owner: -
--

CREATE UNIQUE INDEX ix_download_links_token ON public.download_links USING btree (token);


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
-- Name: ix_sync_jobs_frameio_file_id; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX ix_sync_jobs_frameio_file_id ON public.sync_jobs USING btree (frameio_file_id);


--
-- Name: ix_sync_jobs_sync_rule_id; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX ix_sync_jobs_sync_rule_id ON public.sync_jobs USING btree (sync_rule_id);


--
-- Name: ix_upload_links_destination_id; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX ix_upload_links_destination_id ON public.upload_links USING btree (destination_id);


--
-- Name: ix_upload_links_token; Type: INDEX; Schema: public; Owner: -
--

CREATE UNIQUE INDEX ix_upload_links_token ON public.upload_links USING btree (token);


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
    ADD CONSTRAINT download_sessions_download_link_id_fkey FOREIGN KEY (download_link_id) REFERENCES public.download_links(id) ON DELETE CASCADE;


--
-- Name: frameio_connections frameio_connections_user_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.frameio_connections
    ADD CONSTRAINT frameio_connections_user_id_fkey FOREIGN KEY (user_id) REFERENCES public.users(id) ON DELETE CASCADE;


--
-- Name: sync_jobs sync_jobs_sync_rule_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.sync_jobs
    ADD CONSTRAINT sync_jobs_sync_rule_id_fkey FOREIGN KEY (sync_rule_id) REFERENCES public.sync_rules(id) ON DELETE CASCADE;


--
-- Name: upload_links upload_links_destination_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.upload_links
    ADD CONSTRAINT upload_links_destination_id_fkey FOREIGN KEY (destination_id) REFERENCES public.destinations(id) ON DELETE CASCADE;


--
-- Name: upload_sessions upload_sessions_upload_link_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.upload_sessions
    ADD CONSTRAINT upload_sessions_upload_link_id_fkey FOREIGN KEY (upload_link_id) REFERENCES public.upload_links(id) ON DELETE CASCADE;


--
-- Name: webauthn_credentials webauthn_credentials_user_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.webauthn_credentials
    ADD CONSTRAINT webauthn_credentials_user_id_fkey FOREIGN KEY (user_id) REFERENCES public.users(id) ON DELETE CASCADE;


--
-- PostgreSQL database dump complete
--

\unrestrict tfbENkYsaCXcqiOD94YqXLoWlNWqg8vP3gLXERIJj6Hihj7SNxiEVvfqqtIgmAz

