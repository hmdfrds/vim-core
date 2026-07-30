// Scenario fidelity tests: Sysadmin Workflows
//
// Realistic editing sequences a systems administrator performs daily when
// SSH'd into servers: editing nginx/apache configs, shell scripts, log files,
// YAML manifests, INI configs, /etc/hosts, Dockerfiles, .env files, and
// performing batch operations with ex commands and visual block mode.

// =============================================================================
// NGINX CONFIGURATION
// =============================================================================

// Change nginx listen port from 80 to 8080
neovim_test!(scenarios, sysadmin_nginx_change_listen_port,
    "server {\n    listen 80;\n    server_name example.com;\n}",
    cursor(1, 0), "f8ciw8080<Esc>");

// Add a new location block after existing one
neovim_test!(scenarios, sysadmin_nginx_add_location_block,
    "server {\n    location / {\n        root /var/www/html;\n    }\n}",
    cursor(3, 0), "o\n    location /api {\n        proxy_pass http://127.0.0.1:3000;\n    }<Esc>");

// Change proxy_pass backend host
neovim_test!(scenarios, sysadmin_nginx_change_proxy_host,
    "location /app {\n    proxy_pass http://localhost:5000;\n}",
    cursor(1, 0), "/localhost<CR>ciw10.0.0.5<Esc>");

// Comment out a server_name directive
neovim_test!(scenarios, sysadmin_nginx_comment_directive,
    "server {\n    server_name old.example.com;\n    listen 443 ssl;\n}",
    cursor(1, 0), "I# <Esc>");

// Replace ssl_certificate path
neovim_test!(scenarios, sysadmin_nginx_change_ssl_cert_path,
    "ssl_certificate /etc/ssl/certs/old.pem;\nssl_certificate_key /etc/ssl/private/old.key;",
    "f/ct;/etc/letsencrypt/live/example.com/fullchain.pem<Esc>");

// Duplicate a server block's listen directive and change to ssl
neovim_test!(scenarios, sysadmin_nginx_dup_listen_add_ssl,
    "server {\n    listen 80;\n    server_name app.io;\n}",
    cursor(1, 0), "yypf8ciw443<Esc>A ssl<Esc>");

// =============================================================================
// SHELL SCRIPT EDITING
// =============================================================================

// Add set -euo pipefail at top of script
neovim_test!(scenarios, sysadmin_shell_add_strict_mode,
    "#!/bin/bash\n\necho \"starting backup\"",
    cursor(0, 0), "oset -euo pipefail<Esc>");

// Comment out a dangerous rm line
neovim_test!(scenarios, sysadmin_shell_comment_rm_line,
    "#!/bin/bash\nrm -rf /tmp/build/*\necho done",
    cursor(1, 0), "I# <Esc>");

// Add error handling after a command (|| exit 1)
neovim_test!(scenarios, sysadmin_shell_add_error_handling,
    "#!/bin/bash\ncp /etc/config /backup/config\necho done",
    cursor(1, 0), "A || exit 1<Esc>");

// Change a variable value in a shell script
neovim_test!(scenarios, sysadmin_shell_change_variable,
    "#!/bin/bash\nBACKUP_DIR=\"/old/backup\"\ntar czf $BACKUP_DIR/archive.tar.gz /data",
    cursor(1, 0), "f\"ci\"/mnt/nfs/backup<Esc>");

// Wrap a command in an if block
neovim_test!(scenarios, sysadmin_shell_wrap_in_if,
    "systemctl restart nginx",
    "Oif systemctl is-active nginx; then<Esc>jofi<Esc>k>>>");

// Add a new function to a shell script
neovim_test!(scenarios, sysadmin_shell_add_function,
    "#!/bin/bash\n\ncleanup() {\n    rm -f /tmp/*.log\n}\n\ncleanup",
    cursor(5, 0), "Obackup() {\n    tar czf /backup/data.tar.gz /var/data\n}<Esc>");

// =============================================================================
// LOG ANALYSIS
// =============================================================================

// Search for ERROR in logs and delete the noise line above it
neovim_test!(scenarios, sysadmin_log_search_error,
    "INFO: request started\nDEBUG: parsing headers\nERROR: connection refused\nINFO: retrying",
    "/ERROR<CR>");

// Delete INFO noise lines from a log extract
neovim_test!(scenarios, sysadmin_log_delete_info_lines,
    "INFO: ok\nERROR: fail\nINFO: ok\nWARN: slow\nINFO: ok",
    ":g/INFO/d<CR>");

// Delete all lines NOT matching ERROR
neovim_test!(scenarios, sysadmin_log_keep_only_errors,
    "INFO: startup\nERROR: disk full\nINFO: ok\nERROR: timeout\nINFO: shutdown",
    ":v/ERROR/d<CR>");

// Navigate through search results with n
neovim_test!(scenarios, sysadmin_log_search_next,
    "INFO: ok\nERROR: first\nINFO: ok\nERROR: second\nINFO: ok\nERROR: third",
    "/ERROR<CR>nn");

// Delete the current error line and jump to next error
neovim_test!(scenarios, sysadmin_log_delete_and_next,
    "ERROR: noise\nERROR: real issue\nERROR: noise",
    "/ERROR<CR>ddn");

// =============================================================================
// YAML EDITING (Kubernetes manifests, Ansible playbooks, etc.)
// =============================================================================

// Indent a block deeper with >>
neovim_test!(scenarios, sysadmin_yaml_indent_block,
    "spec:\ncontainers:\n- name: app\n  image: nginx",
    cursor(1, 0), ">>j>>j>>");

// Change a container image tag
neovim_test!(scenarios, sysadmin_yaml_change_image_tag,
    "spec:\n  containers:\n  - name: web\n    image: nginx:1.21",
    cursor(3, 0), "f:lC1.25<Esc>");

// Add a new environment variable to a pod spec
neovim_test!(scenarios, sysadmin_yaml_add_env_var,
    "    env:\n    - name: DB_HOST\n      value: postgres",
    cursor(2, 0), "o    - name: DB_PORT\n      value: \"5432\"<Esc>");

// Change replicas count
neovim_test!(scenarios, sysadmin_yaml_change_replicas,
    "apiVersion: apps/v1\nkind: Deployment\nmetadata:\n  name: web\nspec:\n  replicas: 1",
    cursor(5, 0), "f1r3");

// Add a label to metadata
neovim_test!(scenarios, sysadmin_yaml_add_label,
    "metadata:\n  name: my-service\n  labels:\n    app: web",
    cursor(3, 0), "o    tier: frontend<Esc>");

// Outdent an over-indented YAML key
neovim_test!(scenarios, sysadmin_yaml_outdent,
    "spec:\n      replicas: 3\n      selector:\n        matchLabels:\n          app: web",
    cursor(1, 0), "<<j<<");

// =============================================================================
// INI FILE EDITING
// =============================================================================

// Add a new section to an INI file
neovim_test!(scenarios, sysadmin_ini_add_section,
    "[database]\nhost = localhost\nport = 5432\n\n[logging]\nlevel = info",
    cursor(3, 0), "o[cache]\nbackend = redis\nhost = 127.0.0.1<Esc>");

// Change a value in an INI file
neovim_test!(scenarios, sysadmin_ini_change_value,
    "[server]\nport = 8080\nworkers = 4",
    cursor(1, 0), "f=lcW9090<Esc>");

// Comment out an entire INI section (3 lines)
neovim_test!(scenarios, sysadmin_ini_comment_section,
    "[debug]\nenable = true\nverbose = true\n\n[production]\nenable = false",
    cursor(0, 0), "I; <Esc>jI; <Esc>jI; <Esc>");

// =============================================================================
// /etc/hosts EDITING
// =============================================================================

// Add a new hosts entry at the end
neovim_test!(scenarios, sysadmin_hosts_add_entry,
    "127.0.0.1   localhost\n::1         localhost\n192.168.1.1 gateway",
    "Go10.0.0.50    db-primary.internal<Esc>");

// Change an IP address in /etc/hosts
neovim_test!(scenarios, sysadmin_hosts_change_ip,
    "192.168.1.10  web-01.internal\n192.168.1.11  web-02.internal",
    cursor(0, 0), "cW10.0.0.10<Esc>");

// Duplicate a host entry and change the hostname
neovim_test!(scenarios, sysadmin_hosts_dup_and_modify,
    "10.0.0.10  app-01.prod\n10.0.0.11  app-02.prod",
    cursor(1, 0), "yypf1ciw12<Esc>f-lciwapp-03<Esc>");

// =============================================================================
// DOCKERFILE EDITING
// =============================================================================

// Add an ENV line to a Dockerfile
neovim_test!(scenarios, sysadmin_docker_add_env,
    "FROM ubuntu:22.04\nRUN apt-get update\nCMD [\"/bin/bash\"]",
    cursor(1, 0), "oENV DEBIAN_FRONTEND=noninteractive<Esc>");

// Reorder RUN commands with ddp
neovim_test!(scenarios, sysadmin_docker_reorder_run,
    "FROM node:20\nRUN npm install\nRUN npm ci\nCMD [\"node\", \"app.js\"]",
    cursor(1, 0), "ddp");

// Change the base image tag
neovim_test!(scenarios, sysadmin_docker_change_base_image,
    "FROM python:3.11-slim\nWORKDIR /app\nCOPY . .",
    "f:lC3.12-slim<Esc>");

// Add a COPY before the RUN
neovim_test!(scenarios, sysadmin_docker_add_copy_before_run,
    "FROM golang:1.22\nRUN go build -o app .\nCMD [\"./app\"]",
    cursor(1, 0), "OCOPY go.mod go.sum ./<Esc>");

// =============================================================================
// .env FILE EDITING
// =============================================================================

// Change a quoted value in a .env file with ci"
neovim_test!(scenarios, sysadmin_env_change_quoted_value,
    "DATABASE_URL=\"postgres://localhost:5432/mydb\"\nREDIS_URL=\"redis://localhost:6379\"",
    cursor(0, 0), "f\"ci\"postgres://db.internal:5432/production<Esc>");

// Change an unquoted value with cW
neovim_test!(scenarios, sysadmin_env_change_unquoted_value,
    "PORT=3000\nHOST=0.0.0.0\nDEBUG=true",
    cursor(2, 0), "f=lcWfalse<Esc>");

// Add a new env variable
neovim_test!(scenarios, sysadmin_env_add_variable,
    "APP_NAME=myservice\nAPP_PORT=8080",
    "GoAPP_SECRET=changeme<Esc>");

// =============================================================================
// BATCH OPERATIONS WITH EX COMMANDS
// =============================================================================

// Sort lines in a config (e.g., sorting package list)
neovim_test!(scenarios, sysadmin_sort_package_list,
    "nginx\ncurl\nwget\nbash\ngit",
    ":%sort<CR>");

// Delete all comment lines from a config
neovim_test!(scenarios, sysadmin_delete_comment_lines,
    "# max connections\nmax_conn = 100\n# timeout\ntimeout = 30\n# retry\nretry = 3",
    ":g/^#/d<CR>");

// Global search and replace port number
neovim_test!(scenarios, sysadmin_replace_port_globally,
    "listen 8080;\nproxy_pass http://backend:8080;\n# old port: 8080",
    ":%s/8080/9090/g<CR>");

// Number all lines (prepend line numbers) -- using a substitute
neovim_test!(scenarios, sysadmin_sub_add_prefix,
    "server1\nserver2\nserver3",
    ":%s/^/host: /g<CR>");

// Delete blank lines from a config
neovim_test!(scenarios, sysadmin_delete_blank_lines,
    "[section1]\nkey = val\n\n\n[section2]\nkey2 = val2",
    ":g/^$/d<CR>");

// =============================================================================
// VISUAL BLOCK COMMENTING
// =============================================================================

// Block comment 3 lines in a config
neovim_test!(scenarios, sysadmin_vblock_comment_three_lines,
    "listen 80;\nserver_name app;\nroot /var/www;",
    "<C-v>jjI# <Esc>");

// Block uncomment 3 lines (delete first 2 chars)
neovim_test!(scenarios, sysadmin_vblock_uncomment_three_lines,
    "# listen 80;\n# server_name app;\n# root /var/www;",
    "<C-v>jjlx");

// =============================================================================
// SEARCH AND REPLACE IN CONFIG
// =============================================================================

// Change all occurrences of old domain to new
neovim_test!(scenarios, sysadmin_replace_domain,
    "server_name old.example.com;\nproxy_set_header Host old.example.com;\n# old.example.com",
    ":%s/old\\.example\\.com/new.example.io/g<CR>");

// =============================================================================
// DUPLICATE AND MODIFY PATTERNS
// =============================================================================

// Duplicate an upstream server entry and change the port
neovim_test!(scenarios, sysadmin_dup_upstream_entry,
    "upstream backend {\n    server 10.0.0.1:8080;\n    server 10.0.0.2:8080;\n}",
    cursor(2, 0), "yypf2r3");

// Duplicate an iptables rule and change the port
neovim_test!(scenarios, sysadmin_dup_iptables_rule,
    "-A INPUT -p tcp --dport 22 -j ACCEPT\n-A INPUT -p tcp --dport 80 -j ACCEPT",
    cursor(1, 0), "yypf8ciw443<Esc>");

// Duplicate a crontab line and change the schedule
neovim_test!(scenarios, sysadmin_dup_crontab_entry,
    "0 2 * * * /usr/local/bin/backup.sh\n0 6 * * * /usr/local/bin/cleanup.sh",
    cursor(0, 0), "yypr6");

// =============================================================================
// MULTI-STEP REALISTIC WORKFLOWS
// =============================================================================

// SSH config: add a new host block by duplicating and editing
neovim_test!(scenarios, sysadmin_ssh_config_new_host,
    "Host prod-web-01\n    HostName 10.0.1.10\n    User deploy\n    Port 22",
    "yy3jpciWprod-web-02<Esc>jf0lciw2.20<Esc>");

// systemd unit: change the ExecStart binary path
neovim_test!(scenarios, sysadmin_systemd_change_exec,
    "[Service]\nExecStart=/usr/bin/old-daemon --config /etc/old.conf\nRestart=always",
    cursor(1, 0), "f/ct /usr/local/bin/new-daemon --config /etc/new.conf<Esc>");

// Apache vhost: change DocumentRoot and Directory together
neovim_test!(scenarios, sysadmin_apache_change_docroot,
    "<VirtualHost *:80>\n    DocumentRoot /var/www/old-site\n    <Directory /var/www/old-site>\n</VirtualHost>",
    ":%s/old-site/new-site/g<CR>");

// Logrotate config: change rotation count
neovim_test!(scenarios, sysadmin_logrotate_change_rotate,
    "/var/log/app/*.log {\n    daily\n    rotate 7\n    compress\n    missingok\n}",
    cursor(2, 0), "f7r30");
